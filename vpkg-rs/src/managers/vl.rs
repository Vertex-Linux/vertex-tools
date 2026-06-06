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

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    /// Hosted inside the vpkg-repo (default).
    #[default]
    Repo,
    /// Fetched from a GitHub repository's releases (or cloned if no asset found).
    Github,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InstallType {
    /// `sudo pacman -U <file>` — for pre-built `.pkg.tar.zst` packages.
    Pacman,
    /// Unzip / clone, find a PKGBUILD, run `makepkg -si`.
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

    // ── GitHub source ─────────────────────────────────────────────────────────
    #[serde(default)]
    pub source: DownloadSource,

    /// `owner/repo` — required when source = "github".
    pub github_repo: Option<String>,

    /// Asset filename to download. Use `{arch}` for substitution.
    /// If omitted or the asset isn't found in the release, vpkg will
    /// `git clone` the repo and run commands from there instead.
    pub github_asset: Option<String>,

    /// If set, find the latest release whose tag *contains* this string.
    /// If omitted, the very latest release is used.
    pub github_tag_keyword: Option<String>,

    // ── Rename ────────────────────────────────────────────────────────────────
    /// For `binary` installs: rename the downloaded/cloned file to this before
    /// placing it in `/usr/local/bin/`. Falls back to `name` if omitted.
    #[serde(rename = "rename-file")]
    pub rename_file: Option<String>,

    // ── Desktop entry ─────────────────────────────────────────────────────────
    /// If true, create a .desktop file in /usr/share/applications/ after install.
    #[serde(default, rename = "create-desktop")]
    pub create_desktop: bool,

    /// Human-readable app name shown in launchers. Defaults to `name`.
    #[serde(rename = "desktop-name")]
    pub desktop_name: Option<String>,

    /// Short description shown in launchers. Defaults to `description`.
    #[serde(rename = "desktop-comment")]
    pub desktop_comment: Option<String>,

    /// XDG categories string, e.g. `"Utility;System;"`. Defaults to `"Application;"`.
    #[serde(rename = "desktop-categories")]
    pub desktop_categories: Option<String>,

    /// Command used in the Exec= field. Defaults to `rename-file` or `name`.
    #[serde(rename = "desktop-exec")]
    pub desktop_exec: Option<String>,

    /// Icon filename inside the package's repo folder (e.g. `"myapp.png"`).
    /// Fetched from the vpkg-repo and installed to /usr/share/pixmaps/.
    pub icon: Option<String>,

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

    // ── Repo helpers ──────────────────────────────────────────────────────────

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

    // ── GitHub helpers ────────────────────────────────────────────────────────

    async fn resolve_gh_release(
        &self,
        repo: &str,
        tag_keyword: Option<&str>,
    ) -> Result<GhRelease> {
        match tag_keyword {
            None => {
                let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
                self.client
                    .get(&url)
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<GhRelease>()
                    .await
                    .context("Failed to parse GitHub release")
            }
            Some(keyword) => {
                let url = format!("https://api.github.com/repos/{}/releases", repo);
                let releases: Vec<GhRelease> =
                    self.client.get(&url).send().await?.error_for_status()?.json().await?;
                releases
                    .into_iter()
                    .find(|r| r.tag_name.contains(keyword))
                    .with_context(|| {
                        format!("No release with tag containing '{}' in {}", keyword, repo)
                    })
            }
        }
    }

    /// Try to find a specific release asset and return its download URL.
    /// Returns `None` if the release or the asset doesn't exist (soft failure).
    async fn try_find_asset(
        &self,
        repo: &str,
        asset_name: &str,
        tag_keyword: Option<&str>,
    ) -> Option<(String /* url */, String /* filename */)> {
        let release = self.resolve_gh_release(repo, tag_keyword).await.ok()?;
        let asset = release.assets.iter().find(|a| a.name == asset_name)?;
        Some((asset.browser_download_url.clone(), asset_name.to_string()))
    }

    // ── Install dispatch ──────────────────────────────────────────────────────

    async fn download_and_install(&self, pkg_name: &str) -> Result<()> {
        let meta = self.fetch_pkg_json(pkg_name).await?;

        let tmp_dir = format!("/tmp/vpkg-vl/{}", pkg_name);
        let tmp_path = PathBuf::from(&tmp_dir);
        if tmp_path.exists() {
            fs::remove_dir_all(&tmp_path)?;
        }
        fs::create_dir_all(&tmp_path)?;

        if meta.source == DownloadSource::Github {
            let repo = meta
                .github_repo
                .as_deref()
                .with_context(|| format!("pkg.json for '{}' is missing 'github_repo'", pkg_name))?;

            // Try to resolve a release asset when one is specified
            let asset_result = if let Some(tmpl) = &meta.github_asset {
                let asset_name = tmpl.replace("{arch}", std::env::consts::ARCH);
                self.try_find_asset(repo, &asset_name, meta.github_tag_keyword.as_deref()).await
            } else {
                None
            };

            match asset_result {
                Some((url, filename)) => {
                    let dest = tmp_path.join(&filename);
                    println!(
                        "\x1b[32m==> Downloading {} {} from GitHub release…\x1b[0m",
                        meta.name, meta.version
                    );
                    let bytes = self
                        .client
                        .get(&url)
                        .send()
                        .await
                        .context("Download failed")?
                        .error_for_status()
                        .context("Server error during download")?
                        .bytes()
                        .await?;
                    fs::write(&dest, &bytes)?;
                    println!("  {} downloaded ({} KB)", filename, bytes.len() / 1024);
                    self.run_install(&meta, &dest, &tmp_path).await?;
                }
                None => {
                    self.install_from_git_clone(&meta, &tmp_path, repo).await?;
                }
            }

            if meta.create_desktop {
                self.install_desktop_entry(&meta, pkg_name, &tmp_path).await?;
            }
            return Ok(());
        }

        // Repo-hosted source
        let file = meta.file.as_deref().with_context(|| {
            format!("pkg.json for '{}' is missing the 'file' field", pkg_name)
        })?;
        let url = format!("{}/{}/{}", REPO_RAW, pkg_name, file);
        let dest = tmp_path.join(file);
        println!(
            "\x1b[32m==> Downloading {} {} from VL repo…\x1b[0m",
            meta.name, meta.version
        );
        let bytes = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to download package file")?
            .error_for_status()
            .with_context(|| format!("File '{}' not found in VL repo", file))?
            .bytes()
            .await?;
        fs::write(&dest, &bytes)?;
        println!("  {} downloaded ({} KB)", file, bytes.len() / 1024);
        self.run_install(&meta, &dest, &tmp_path).await?;
        if meta.create_desktop {
            self.install_desktop_entry(&meta, pkg_name, &tmp_path).await?;
        }
        Ok(())
    }

    async fn install_desktop_entry(
        &self,
        meta: &PkgJson,
        pkg_name: &str,
        tmp_path: &PathBuf,
    ) -> Result<()> {
        println!("\x1b[32m==> Creating desktop entry…\x1b[0m");

        let app_name = meta.desktop_name.as_deref().unwrap_or(&meta.name);
        let comment = meta
            .desktop_comment
            .as_deref()
            .or(meta.description.as_deref())
            .unwrap_or("");
        let categories = meta.desktop_categories.as_deref().unwrap_or("Application;");
        let exec = meta
            .desktop_exec
            .as_deref()
            .or(meta.rename_file.as_deref())
            .unwrap_or(&meta.name);

        // Download and install icon if one is specified in pkg.json
        let icon_value = if let Some(icon_file) = &meta.icon {
            let icon_url = format!("{}/{}/{}", REPO_RAW, pkg_name, icon_file);
            let icon_bytes = self
                .client
                .get(&icon_url)
                .send()
                .await
                .context("Failed to fetch icon from repo")?
                .error_for_status()
                .with_context(|| format!("Icon '{}' not found in VL repo", icon_file))?
                .bytes()
                .await?;

            let tmp_icon = tmp_path.join(icon_file);
            fs::write(&tmp_icon, &icon_bytes)?;

            let icon_dest = format!("/usr/share/pixmaps/{}", icon_file);
            let status = Command::new("pkexec")
                .args([
                    "install",
                    "-m",
                    "644",
                    tmp_icon.to_str().unwrap_or(""),
                    &icon_dest,
                ])
                .status()
                .await
                .context("pkexec not found")?;
            if !status.success() {
                anyhow::bail!("Failed to install icon to {}", icon_dest);
            }
            println!("  Icon installed → {}", icon_dest);
            icon_dest
        } else {
            // No icon specified — reference by app name so the DE can look it up in its icon theme
            meta.name.clone()
        };

        let desktop_content = format!(
            "[Desktop Entry]\nName={}\nComment={}\nExec={}\nIcon={}\nType=Application\nCategories={}\n",
            app_name, comment, exec, icon_value, categories
        );

        let desktop_filename = format!("{}.desktop", meta.name);
        let tmp_desktop = tmp_path.join(&desktop_filename);
        fs::write(&tmp_desktop, &desktop_content)?;

        let dest = format!("/usr/share/applications/{}", desktop_filename);
        let status = Command::new("pkexec")
            .args([
                "install",
                "-m",
                "644",
                tmp_desktop.to_str().unwrap_or(""),
                &dest,
            ])
            .status()
            .await
            .context("pkexec not found")?;
        if !status.success() {
            anyhow::bail!("Failed to install desktop file to {}", dest);
        }
        println!("  Desktop entry installed → {}", dest);
        Ok(())
    }

    /// Install a file that has already been downloaded to `dest`.
    async fn run_install(&self, meta: &PkgJson, dest: &PathBuf, tmp_path: &PathBuf) -> Result<()> {
        match meta.install_type {
            InstallType::Pacman => {
                println!("\x1b[32m==> Installing via pkexec pacman -U…\x1b[0m");
                let status = Command::new("pkexec")
                    .args(["pacman", "-U", "--noconfirm"])
                    .arg(dest)
                    .status()
                    .await
                    .context("pkexec not found")?;
                if !status.success() {
                    anyhow::bail!("pacman -U failed for '{}'", meta.name);
                }
            }
            InstallType::Makepkg => {
                println!("\x1b[32m==> Extracting {}…\x1b[0m", dest.display());
                let status = Command::new("unzip")
                    .args(["-q", dest.to_str().unwrap_or(""), "-d", tmp_path.to_str().unwrap_or("")])
                    .status()
                    .await
                    .context("unzip not found — install with: vpkg pm install unzip")?;
                if !status.success() {
                    anyhow::bail!("Failed to extract archive");
                }
                self.run_makepkg(tmp_path).await?;
            }
            InstallType::Binary => {
                let bin_name = meta.rename_file.as_deref().unwrap_or(&meta.name);
                let dest_bin = format!("/usr/local/bin/{}", bin_name);
                println!("\x1b[32m==> Installing binary → {}…\x1b[0m", dest_bin);
                let status = Command::new("pkexec")
                    .args(["install", "-m", "755", dest.to_str().unwrap_or(""), &dest_bin])
                    .status()
                    .await
                    .context("pkexec not found")?;
                if !status.success() {
                    anyhow::bail!("Failed to install binary to {}", dest_bin);
                }
                println!("  Installed {} → {}", bin_name, dest_bin);
            }
        }
        Ok(())
    }

    /// Clone the GitHub repo and install directly from the repo root.
    /// Used when no `github_asset` is specified or the asset isn't found in any release.
    async fn install_from_git_clone(
        &self,
        meta: &PkgJson,
        tmp_path: &PathBuf,
        repo: &str,
    ) -> Result<()> {
        let clone_url = format!("https://github.com/{}.git", repo);
        let clone_dir = tmp_path.join("repo");

        println!(
            "\x1b[32m==> No release asset — cloning {} …\x1b[0m",
            repo
        );
        let status = Command::new("git")
            .args([
                "clone",
                "--depth=1",
                &clone_url,
                clone_dir.to_str().unwrap_or(""),
            ])
            .status()
            .await
            .context("git not found")?;
        if !status.success() {
            anyhow::bail!("Failed to clone {}", clone_url);
        }

        match meta.install_type {
            InstallType::Makepkg => {
                self.run_makepkg(&clone_dir).await?;
            }
            InstallType::Binary => {
                let bin_name = meta.rename_file.as_deref().unwrap_or(&meta.name);
                // Look for the binary in the repo root
                let bin_in_repo = clone_dir.join(bin_name);
                if !bin_in_repo.exists() {
                    anyhow::bail!(
                        "Binary '{}' not found in cloned repo root of {}",
                        bin_name,
                        repo
                    );
                }
                let dest_bin = format!("/usr/local/bin/{}", bin_name);
                println!("\x1b[32m==> Installing binary → {}…\x1b[0m", dest_bin);
                let status = Command::new("pkexec")
                    .args([
                        "install",
                        "-m",
                        "755",
                        bin_in_repo.to_str().unwrap_or(""),
                        &dest_bin,
                    ])
                    .status()
                    .await?;
                if !status.success() {
                    anyhow::bail!("Failed to install binary to {}", dest_bin);
                }
                println!("  Installed {} → {}", bin_name, dest_bin);
            }
            InstallType::Pacman => {
                anyhow::bail!(
                    "pacman install type requires a release asset — \
                     add 'github_asset' to pkg.json or upload a release"
                );
            }
        }
        Ok(())
    }

    /// Build with makepkg (as current user) then install via pkexec pacman -U
    /// (graphical polkit prompt — no terminal required).
    async fn run_makepkg(&self, search_root: &PathBuf) -> Result<()> {
        let build_dir = find_pkgbuild_dir(search_root)?;

        // Build only — makepkg refuses to run as root so we cannot wrap this in pkexec.
        println!("\x1b[32m==> Building with makepkg…\x1b[0m");
        let status = Command::new("makepkg")
            .args(["-s", "--noconfirm"])
            .current_dir(&build_dir)
            .status()
            .await
            .context("makepkg not found")?;
        if !status.success() {
            anyhow::bail!("makepkg build failed in {}", build_dir.display());
        }

        // Find the generated package archive.
        let pkg_file = find_built_package(&build_dir)?;

        // Install via pkexec — triggers the graphical polkit authentication dialog.
        println!("\x1b[32m==> Installing via pkexec pacman -U…\x1b[0m");
        let status = Command::new("pkexec")
            .args(["pacman", "-U", "--noconfirm"])
            .arg(&pkg_file)
            .status()
            .await
            .context("pkexec not found")?;
        if !status.success() {
            anyhow::bail!("Package installation failed in {}", build_dir.display());
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
    anyhow::bail!("No PKGBUILD found in {}", root.display())
}

fn find_built_package(dir: &PathBuf) -> Result<PathBuf> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".pkg.tar.zst") || name.ends_with(".pkg.tar.xz") {
            return Ok(path);
        }
    }
    anyhow::bail!("No .pkg.tar.zst file found in {}", dir.display())
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
                let meta = client.get(&url).send().await.ok()?.json::<PkgJson>().await.ok()?;
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
            let is_binary = self
                .fetch_pkg_json(pkg)
                .await
                .map(|m| m.install_type == InstallType::Binary)
                .unwrap_or(false);

            if is_binary {
                let path = format!("/usr/local/bin/{}", pkg);
                println!("\x1b[32m==> Removing {}…\x1b[0m", path);
                let status = Command::new("pkexec")
                    .args(["rm", "-f", &path])
                    .status()
                    .await
                    .context("pkexec not found")?;
                if !status.success() {
                    anyhow::bail!("Failed to remove '{}'", path);
                }
            } else {
                let status = Command::new("pkexec")
                    .args(["pacman", "-Rs", "--noconfirm", pkg])
                    .status()
                    .await
                    .context("pkexec not found")?;
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
