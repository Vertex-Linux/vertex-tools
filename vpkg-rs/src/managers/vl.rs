use crate::managers::Manager;
use crate::package::{Package, Source};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use tokio::process::Command;

const REPO_API: &str = "https://api.github.com/repos/Vertex-Linux/vpkg-repo/contents";
const REPO_RAW: &str = "https://raw.githubusercontent.com/Vertex-Linux/vpkg-repo/main";

// ── Repo listing ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RepoEntry {
    name: String,
    #[serde(rename = "type")]
    entry_type: String,
}

// ── GitHub releases ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

// ── pkg.json schema ───────────────────────────────────────────────────────────

/// Where to download the package from.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    /// Hosted inside the vpkg-repo (default).
    Repo,
    /// Fetched from a GitHub repository's releases.
    Github,
}

impl Default for DownloadSource {
    fn default() -> Self {
        DownloadSource::Repo
    }
}

/// How to install the downloaded file.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InstallType {
    /// `sudo pacman -U <file>` — for pre-built `.pkg.tar.zst` packages.
    Pacman,
    /// Unzip the archive, find a PKGBUILD, run `makepkg -si`.
    Makepkg,
    /// Copy the file directly to `/usr/local/bin/<name>` and `chmod +x`.
    Binary,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PkgJson {
    pub name: String,
    pub version: String,
    pub description: Option<String>,

    #[serde(rename = "type")]
    pub install_type: InstallType,

    // ── Repo-hosted source ────────────────────────────────────────────────────
    /// Filename inside the package folder (required when source = "repo").
    pub file: Option<String>,

    // ── GitHub release source ─────────────────────────────────────────────────
    #[serde(default)]
    pub source: DownloadSource,

    /// `owner/repo` — required when source = "github".
    pub github_repo: Option<String>,

    /// Asset filename to download. Use `{arch}` for automatic substitution
    /// (e.g. `"mytool-{arch}-unknown-linux-gnu"`).
    pub github_asset: Option<String>,

    /// If set, find the latest release whose tag *contains* this keyword.
    /// If omitted, the very latest release is used.
    pub github_tag_keyword: Option<String>,

    /// For `binary` installs: rename the downloaded file to this name before
    /// placing it in `/usr/local/bin/`. Falls back to `name` if omitted.
    #[serde(rename = "rename-file")]
    pub rename_file: Option<String>,

    // ── Dependencies ──────────────────────────────────────────────────────────
    #[serde(default)]
    pub pm: Vec<String>,
    #[serde(default)]
    pub aur: Vec<String>,
    #[serde(default)]
    pub fp: Vec<String>,
}

// ── VlManager ─────────────────────────────────────────────────────────────────

pub struct VlManager {
    pub client: Client,
}

impl VlManager {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("vpkg/0.1 (Vertex Package Manager)")
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    async fn list_package_names(&self) -> Result<Vec<String>> {
        let entries: Vec<RepoEntry> = self
            .client
            .get(REPO_API)
            .send()
            .await
            .context("Failed to reach Vertex Linux package repo")?
            .error_for_status()
            .context("Vertex Linux repo returned an error")?
            .json()
            .await?;

        Ok(entries
            .into_iter()
            .filter(|e| e.entry_type == "dir")
            .map(|e| e.name)
            .collect())
    }

    /// Fetch and parse the `pkg.json` for a package by its folder name.
    pub async fn fetch_pkg_json(&self, pkg_name: &str) -> Result<PkgJson> {
        let url = format!("{}/{}/pkg.json", REPO_RAW, pkg_name);
        self.client
            .get(&url)
            .send()
            .await
            .context("Network error fetching pkg.json")?
            .error_for_status()
            .with_context(|| format!("'{}' not found in Vertex Linux repo", pkg_name))?
            .json::<PkgJson>()
            .await
            .context("Failed to parse pkg.json")
    }

    /// Resolve the download URL and the filename to save as.
    /// For GitHub sources, `{arch}` in `github_asset` is substituted automatically.
    async fn resolve_download(&self, pkg_name: &str, meta: &PkgJson) -> Result<(String, String)> {
        match &meta.source {
            DownloadSource::Repo => {
                let file = meta
                    .file
                    .as_deref()
                    .with_context(|| {
                        format!("pkg.json for '{}' is missing the 'file' field", pkg_name)
                    })?;
                let url = format!("{}/{}/{}", REPO_RAW, pkg_name, file);
                Ok((url, file.to_string()))
            }

            DownloadSource::Github => {
                let repo = meta.github_repo.as_deref().with_context(|| {
                    format!("pkg.json for '{}' is missing 'github_repo'", pkg_name)
                })?;
                let asset_template = meta.github_asset.as_deref().with_context(|| {
                    format!("pkg.json for '{}' is missing 'github_asset'", pkg_name)
                })?;

                // Substitute {arch} → actual architecture
                let arch = std::env::consts::ARCH; // "x86_64" | "aarch64"
                let asset_name = asset_template.replace("{arch}", arch);

                let release = self.resolve_gh_release(repo, meta.github_tag_keyword.as_deref()).await?;

                println!("  Found GitHub release {}", release.tag_name);

                let asset = release
                    .assets
                    .iter()
                    .find(|a| a.name == asset_name)
                    .with_context(|| {
                        format!(
                            "Asset '{}' not found in release {} of {}",
                            asset_name, release.tag_name, repo
                        )
                    })?;

                Ok((asset.browser_download_url.clone(), asset_name))
            }
        }
    }

    /// Fetch the correct GitHub release — either the first one whose tag contains
    /// `tag_keyword`, or the very latest if no keyword is given.
    async fn resolve_gh_release(&self, repo: &str, tag_keyword: Option<&str>) -> Result<GhRelease> {
        match tag_keyword {
            None => {
                // Use the /releases/latest shortcut
                let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
                self.client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to reach GitHub API")?
                    .error_for_status()
                    .context("GitHub API returned an error for releases/latest")?
                    .json::<GhRelease>()
                    .await
                    .context("Failed to parse GitHub release")
            }
            Some(keyword) => {
                let url = format!("https://api.github.com/repos/{}/releases", repo);
                let releases: Vec<GhRelease> = self
                    .client
                    .get(&url)
                    .send()
                    .await
                    .context("Failed to reach GitHub API")?
                    .error_for_status()
                    .context("GitHub API returned an error for releases list")?
                    .json()
                    .await
                    .context("Failed to parse GitHub releases list")?;

                releases
                    .into_iter()
                    .find(|r| r.tag_name.contains(keyword))
                    .with_context(|| {
                        format!(
                            "No release with tag containing '{}' found in {}",
                            keyword, repo
                        )
                    })
            }
        }
    }

    async fn download_and_install(&self, pkg_name: &str) -> Result<()> {
        let meta = self.fetch_pkg_json(pkg_name).await?;

        let tmp_dir = format!("/tmp/vpkg-vl/{}", pkg_name);
        let tmp_path = PathBuf::from(&tmp_dir);
        if tmp_path.exists() {
            fs::remove_dir_all(&tmp_path)?;
        }
        fs::create_dir_all(&tmp_path)?;

        let (download_url, filename) = self.resolve_download(pkg_name, &meta).await?;
        let dest = tmp_path.join(&filename);

        println!(
            "\x1b[32m==> Downloading {} {}…\x1b[0m",
            meta.name, meta.version
        );
        let bytes = self
            .client
            .get(&download_url)
            .send()
            .await
            .context("Failed to download package file")?
            .error_for_status()
            .with_context(|| format!("Download URL returned an error for '{}'", filename))?
            .bytes()
            .await?;
        fs::write(&dest, &bytes)?;
        println!("  {} downloaded ({} KB)", filename, bytes.len() / 1024);

        match meta.install_type {
            InstallType::Pacman => {
                println!("\x1b[32m==> Installing via pacman -U…\x1b[0m");
                let status = Command::new("sudo")
                    .args(["pacman", "-U", "--noconfirm"])
                    .arg(&dest)
                    .status()
                    .await
                    .context("pacman not found")?;
                if !status.success() {
                    anyhow::bail!("pacman -U failed for '{}'", pkg_name);
                }
            }

            InstallType::Makepkg => {
                println!("\x1b[32m==> Extracting {}…\x1b[0m", filename);
                let status = Command::new("unzip")
                    .args(["-q", dest.to_str().unwrap_or(""), "-d", &tmp_dir])
                    .status()
                    .await
                    .context("unzip not found — install it with: vpkg pm install unzip")?;
                if !status.success() {
                    anyhow::bail!("Failed to extract '{}'", filename);
                }

                let build_dir = find_pkgbuild_dir(&tmp_path)?;
                println!("\x1b[32m==> Building with makepkg -si…\x1b[0m");
                let status = Command::new("makepkg")
                    .args(["-si"])
                    .current_dir(&build_dir)
                    .status()
                    .await
                    .context("makepkg not found")?;
                if !status.success() {
                    anyhow::bail!("makepkg failed for '{}'", pkg_name);
                }
            }

            InstallType::Binary => {
                let bin_name = meta.rename_file.as_deref().unwrap_or(&meta.name);
                let dest_bin = format!("/usr/local/bin/{}", bin_name);
                println!(
                    "\x1b[32m==> Installing binary to {}…\x1b[0m",
                    dest_bin
                );
                let status = Command::new("sudo")
                    .args(["install", "-m", "755", dest.to_str().unwrap_or(""), &dest_bin])
                    .status()
                    .await
                    .context("sudo not found")?;
                if !status.success() {
                    anyhow::bail!("Failed to install binary '{}' to {}", bin_name, dest_bin);
                }
                println!("  Installed {} → {}", bin_name, dest_bin);
            }
        }

        Ok(())
    }
}

fn find_pkgbuild_dir(root: &PathBuf) -> Result<PathBuf> {
    if root.join("PKGBUILD").exists() {
        return Ok(root.clone());
    }
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() && path.join("PKGBUILD").exists() {
            return Ok(path);
        }
    }
    anyhow::bail!("No PKGBUILD found in extracted zip archive")
}

// ── Manager trait impl ────────────────────────────────────────────────────────

#[async_trait]
impl Manager for VlManager {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let names = self.list_package_names().await?;
        let query_lower = query.to_lowercase();

        let matching: Vec<String> = names
            .into_iter()
            .filter(|n| query.is_empty() || n.to_lowercase().contains(&query_lower))
            .collect();

        if matching.is_empty() {
            return Ok(Vec::new());
        }

        let client = self.client.clone();
        let futs = matching.into_iter().map(|folder| {
            let client = client.clone();
            async move {
                let url = format!("{}/{}/pkg.json", REPO_RAW, folder);
                let meta = client
                    .get(&url)
                    .send()
                    .await
                    .ok()?
                    .json::<PkgJson>()
                    .await
                    .ok()?;
                Some(Package::new(
                    meta.name,
                    meta.version,
                    meta.description.unwrap_or_default(),
                    Source::VertexLinux,
                ))
            }
        });

        let results = futures::future::join_all(futs).await;
        Ok(results.into_iter().flatten().collect())
    }

    async fn install(&self, packages: &[String]) -> Result<()> {
        for pkg in packages {
            self.download_and_install(pkg).await?;
        }
        Ok(())
    }

    async fn remove(&self, packages: &[String]) -> Result<()> {
        for pkg in packages {
            // Try to read pkg.json to determine if it was a binary install
            let is_binary = self
                .fetch_pkg_json(pkg)
                .await
                .map(|m| m.install_type == InstallType::Binary)
                .unwrap_or(false);

            if is_binary {
                let path = format!("/usr/local/bin/{}", pkg);
                println!("\x1b[32m==> Removing binary {}…\x1b[0m", path);
                let status = Command::new("sudo")
                    .args(["rm", "-f", &path])
                    .status()
                    .await
                    .context("sudo not found")?;
                if !status.success() {
                    anyhow::bail!("Failed to remove '{}'", path);
                }
            } else {
                // Pacman / makepkg packages land in pacman's DB
                let status = Command::new("sudo")
                    .args(["pacman", "-Rs", "--noconfirm", pkg])
                    .status()
                    .await
                    .context("pacman not found")?;
                if !status.success() {
                    anyhow::bail!("Failed to remove '{}'", pkg);
                }
            }
        }
        Ok(())
    }

    async fn update(&self) -> Result<()> {
        println!(
            "\x1b[32m==> To update a VL package, reinstall it: vpkg vl install <name>\x1b[0m"
        );
        Ok(())
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        Ok(Vec::new())
    }
}
