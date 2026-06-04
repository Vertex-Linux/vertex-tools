use crate::managers::Manager;
use crate::package::{Package, Source};
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

pub struct PacmanManager;

impl PacmanManager {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Manager for PacmanManager {
    async fn search(&self, query: &str) -> Result<Vec<Package>> {
        let output = Command::new("pacman")
            .args(["-Ss", query])
            .output()
            .await
            .context("Failed to run pacman -Ss")?;
        parse_search(&String::from_utf8_lossy(&output.stdout))
    }

    async fn install(&self, packages: &[String]) -> Result<()> {
        let status = Command::new("pkexec")
            .args(["pacman", "-S", "--noconfirm"])
            .args(packages)
            .status()
            .await
            .context("pkexec not found")?;
        if !status.success() {
            anyhow::bail!("pacman failed to install packages");
        }
        Ok(())
    }

    async fn remove(&self, packages: &[String]) -> Result<()> {
        let status = Command::new("pkexec")
            .args(["pacman", "-Rs", "--noconfirm"])
            .args(packages)
            .status()
            .await
            .context("pkexec not found")?;
        if !status.success() {
            anyhow::bail!("pacman failed to remove packages");
        }
        Ok(())
    }

    async fn update(&self) -> Result<()> {
        let status = Command::new("pkexec")
            .args(["pacman", "-Syu", "--noconfirm"])
            .status()
            .await
            .context("pkexec not found")?;
        if !status.success() {
            anyhow::bail!("pacman system update failed");
        }
        Ok(())
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        let output = Command::new("pacman")
            .args(["-Qe"])
            .output()
            .await
            .context("Failed to run pacman -Qe")?;
        let mut pkgs = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.splitn(2, ' ');
            if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
                let mut p = Package::new(name, ver.trim(), "", Source::Pacman);
                p.installed = true;
                pkgs.push(p);
            }
        }
        Ok(pkgs)
    }
}

fn parse_search(raw: &str) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    let mut lines = raw.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("    ") || line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let name_part = match parts.next() {
            Some(p) => p,
            None => continue,
        };
        let rest = parts.next().unwrap_or("");
        let name = match name_part.find('/') {
            Some(i) => name_part[i + 1..].to_string(),
            None => name_part.to_string(),
        };
        let version = rest.split_whitespace().next().unwrap_or("").to_string();
        let installed = rest.contains("[installed]");
        let desc = lines.next().map(|l| l.trim().to_string()).unwrap_or_default();
        let mut pkg = Package::new(name, version, desc, Source::Pacman);
        pkg.installed = installed;
        packages.push(pkg);
    }
    Ok(packages)
}
