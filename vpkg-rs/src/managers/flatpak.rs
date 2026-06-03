use crate::managers::Manager;
use crate::package::{Package, Source};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

pub struct FlatpakManager;

impl FlatpakManager {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Manager for FlatpakManager {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let output = Command::new("flatpak")
            .args([
                "search",
                "--columns=application,name,version,description",
                query,
            ])
            .output()
            .await
            .context("flatpak not found")?;
        parse_search(&String::from_utf8_lossy(&output.stdout))
    }

    async fn install(&self, packages: &[String]) -> Result<()> {
        for pkg in packages {
            let status = Command::new("flatpak")
                .args(["install", "--noninteractive", "-y", pkg])
                .status()
                .await
                .context("flatpak not found")?;
            if !status.success() {
                anyhow::bail!("flatpak failed to install '{}'", pkg);
            }
        }
        Ok(())
    }

    async fn remove(&self, packages: &[String]) -> Result<()> {
        let status = Command::new("flatpak")
            .arg("uninstall")
            .arg("-y")
            .args(packages)
            .status()
            .await
            .context("flatpak not found")?;
        if !status.success() {
            anyhow::bail!("flatpak failed to remove packages");
        }
        Ok(())
    }

    async fn update(&self) -> Result<()> {
        let status = Command::new("flatpak")
            .args(["update", "-y"])
            .status()
            .await
            .context("flatpak not found")?;
        if !status.success() {
            anyhow::bail!("flatpak update failed");
        }
        Ok(())
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        let output = Command::new("flatpak")
            .args(["list", "--columns=application,name,version"])
            .output()
            .await
            .context("flatpak not found")?;
        let mut pkgs = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 2 {
                continue;
            }
            let app_id = cols[0].trim().to_string();
            let name = cols[1].trim().to_string();
            let version = cols.get(2).map(|v| v.trim().to_string()).unwrap_or_default();
            if app_id.is_empty() {
                continue;
            }
            let mut p = Package::new(&name, version, "", Source::Flatpak);
            p.app_id = Some(app_id);
            p.installed = true;
            pkgs.push(p);
        }
        Ok(pkgs)
    }
}

fn parse_search(raw: &str) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    for line in raw.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let app_id = cols[0].trim().to_string();
        let name = cols[1].trim().to_string();
        let version = cols.get(2).map(|v| v.trim().to_string()).unwrap_or_default();
        let description = cols.get(3).map(|v| v.trim().to_string()).unwrap_or_default();
        if app_id.is_empty() || name.is_empty() {
            continue;
        }
        let mut pkg = Package::new(&name, &version, description, Source::Flatpak);
        pkg.app_id = Some(app_id);
        packages.push(pkg);
    }
    Ok(packages)
}
