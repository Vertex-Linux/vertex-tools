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
const REPO_RAW: &str =
    "https://raw.githubusercontent.com/Vertex-Linux/vpkg-repo/main";

#[derive(Deserialize)]
struct GithubEntry {
    name: String,
    #[serde(rename = "type")]
    entry_type: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PkgJson {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub install_type: InstallType,
    pub file: String,
    /// Pacman dependencies installed before this package
    #[serde(default)]
    pub pm: Vec<String>,
    /// AUR dependencies installed before this package
    #[serde(default)]
    pub aur: Vec<String>,
    /// Flatpak dependencies installed before this package
    #[serde(default)]
    pub fp: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InstallType {
    Pacman,
    Makepkg,
}

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
        let entries: Vec<GithubEntry> = self
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

    /// Fetch the pkg.json for a package by its folder/install name.
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

    async fn download_and_install(&self, pkg_name: &str) -> Result<()> {
        let meta = self.fetch_pkg_json(pkg_name).await?;

        let tmp_dir = format!("/tmp/vpkg-vl/{}", pkg_name);
        let tmp_path = PathBuf::from(&tmp_dir);
        if tmp_path.exists() {
            fs::remove_dir_all(&tmp_path)?;
        }
        fs::create_dir_all(&tmp_path)?;

        let file_url = format!("{}/{}/{}", REPO_RAW, pkg_name, meta.file);
        let dest = tmp_path.join(&meta.file);

        println!(
            "\x1b[32m==> Downloading {} {} from Vertex Linux repo…\x1b[0m",
            meta.name, meta.version
        );
        let bytes = self
            .client
            .get(&file_url)
            .send()
            .await
            .context("Failed to download package")?
            .error_for_status()
            .with_context(|| format!("Package file '{}' not found", meta.file))?
            .bytes()
            .await?;
        fs::write(&dest, &bytes)?;
        println!("  {} downloaded ({} KB)", meta.file, bytes.len() / 1024);

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
                println!("\x1b[32m==> Extracting {}…\x1b[0m", meta.file);
                let status = Command::new("unzip")
                    .args(["-q", dest.to_str().unwrap_or(""), "-d", &tmp_dir])
                    .status()
                    .await
                    .context("unzip not found — install it with: vpkg pm install unzip")?;
                if !status.success() {
                    anyhow::bail!("Failed to extract '{}'", meta.file);
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

        // Fetch pkg.jsons for matching folders in parallel
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
        // VL packages land in pacman's DB (installed via pacman -U or makepkg)
        let status = Command::new("sudo")
            .args(["pacman", "-Rs", "--noconfirm"])
            .args(packages)
            .status()
            .await
            .context("pacman not found")?;
        if !status.success() {
            anyhow::bail!("Failed to remove VL packages");
        }
        Ok(())
    }

    async fn update(&self) -> Result<()> {
        // VL packages don't have an update feed — reinstall to update
        println!(
            "\x1b[32m==> VL packages are managed by pacman after install.\x1b[0m\n\
             \x1b[32m    Reinstall a package to update: vpkg vl install <name>\x1b[0m"
        );
        Ok(())
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        // Can't distinguish VL packages from regular pacman packages without a manifest
        Ok(Vec::new())
    }
}
