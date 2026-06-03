use crate::managers::Manager;
use crate::package::{Package, Source};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::fs;
use tokio::process::Command;

const AUR_RPC: &str = "https://aur.archlinux.org/rpc/v5";
const AUR_GIT: &str = "https://aur.archlinux.org";

#[derive(Deserialize)]
struct RpcResponse {
    results: Vec<AurPkg>,
}

#[derive(Deserialize, Clone)]
struct AurPkg {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "NumVotes")]
    num_votes: Option<u32>,
    #[serde(rename = "Popularity")]
    popularity: Option<f64>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "PackageBase")]
    package_base: String,
}

impl From<AurPkg> for Package {
    fn from(p: AurPkg) -> Self {
        let mut pkg = Package::new(
            p.name,
            p.version,
            p.description.unwrap_or_default(),
            Source::Aur,
        );
        pkg.votes = p.num_votes;
        pkg.popularity = p.popularity;
        pkg.url = p.url;
        pkg
    }
}

pub struct AurManager {
    client: Client,
}

impl AurManager {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("vpkg2/0.1 (Vertex Package Manager)")
                .build()
                .expect("failed to build HTTP client"),
        }
    }

    async fn pkg_info(&self, name: &str) -> Result<Option<AurPkg>> {
        let url = format!("{}/info?arg[]={}", AUR_RPC, name);
        let resp: RpcResponse = self.client.get(&url).send().await?.json().await?;
        Ok(resp.results.into_iter().next())
    }

    async fn build_and_install(&self, pkg_base: &str) -> Result<()> {
        let build_dir = format!("/tmp/vpkg2-aur/{}", pkg_base);

        if std::path::Path::new(&build_dir).exists() {
            fs::remove_dir_all(&build_dir)?;
        }
        fs::create_dir_all("/tmp/vpkg2-aur")?;

        let clone_url = format!("{}/{}.git", AUR_GIT, pkg_base);
        let status = Command::new("git")
            .args(["clone", "--depth=1", &clone_url, &build_dir])
            .status()
            .await
            .context("git not found — required for AUR builds")?;
        if !status.success() {
            anyhow::bail!("Failed to clone AUR repo for '{}'", pkg_base);
        }

        // Show PKGBUILD
        let pkgbuild = format!("{}/PKGBUILD", build_dir);
        if let Ok(content) = fs::read_to_string(&pkgbuild) {
            println!(
                "\n\x1b[33m══ PKGBUILD for {} ══\x1b[0m\n{}",
                pkg_base, content
            );
        }

        // Confirm
        println!("\n\x1b[33m==> Proceed with build and install? [Y/n]\x1b[0m ");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if line.trim().eq_ignore_ascii_case("n") {
            anyhow::bail!("Cancelled by user");
        }

        let status = Command::new("makepkg")
            .args(["-si"])
            .current_dir(&build_dir)
            .status()
            .await
            .context("makepkg not found")?;
        if !status.success() {
            anyhow::bail!("makepkg failed for '{}'", pkg_base);
        }
        Ok(())
    }
}

#[async_trait]
impl Manager for AurManager {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let url = format!("{}/search/{}?by=name-desc", AUR_RPC, query);
        let resp: RpcResponse = self.client.get(&url).send().await?.json().await?;
        let mut results: Vec<Package> = resp.results.into_iter().map(Package::from).collect();
        results.sort_by(|a, b| {
            b.popularity
                .unwrap_or(0.0)
                .partial_cmp(&a.popularity.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    async fn install(&self, packages: &[String]) -> Result<()> {
        for name in packages {
            let info = self
                .pkg_info(name)
                .await?
                .ok_or_else(|| anyhow::anyhow!("'{}' not found in AUR", name))?;
            self.build_and_install(&info.package_base).await?;
        }
        Ok(())
    }

    async fn remove(&self, packages: &[String]) -> Result<()> {
        let status = Command::new("sudo")
            .arg("pacman")
            .arg("-Rs")
            .arg("--noconfirm")
            .args(packages)
            .status()
            .await?;
        if !status.success() {
            anyhow::bail!("Failed to remove AUR packages");
        }
        Ok(())
    }

    async fn update(&self) -> Result<()> {
        // Identify foreign (AUR) packages, fetch their latest versions, rebuild if outdated
        let output = Command::new("pacman").args(["-Qm"]).output().await?;
        let installed: Vec<(String, String)> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|l| {
                let mut p = l.splitn(2, ' ');
                Some((p.next()?.to_string(), p.next()?.trim().to_string()))
            })
            .collect();

        if installed.is_empty() {
            println!("No AUR packages installed.");
            return Ok(());
        }

        println!("Checking {} AUR packages for updates…", installed.len());
        for (name, local_ver) in installed {
            if let Ok(Some(remote)) = self.pkg_info(&name).await {
                if remote.version != local_ver {
                    println!("  {} {} → {}", name, local_ver, remote.version);
                    self.build_and_install(&remote.package_base).await?;
                }
            }
        }
        Ok(())
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        let output = Command::new("pacman").args(["-Qm"]).output().await?;
        let mut pkgs = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.splitn(2, ' ');
            if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
                let mut p = Package::new(name, ver.trim(), "", Source::Aur);
                p.installed = true;
                pkgs.push(p);
            }
        }
        Ok(pkgs)
    }
}
