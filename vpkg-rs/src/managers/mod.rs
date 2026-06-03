pub mod aur;
pub mod flatpak;
pub mod pacman;

use crate::package::Package;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Manager: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<Package>>;
    async fn install(&self, packages: &[String]) -> Result<()>;
    async fn remove(&self, packages: &[String]) -> Result<()>;
    async fn update(&self) -> Result<()>;
    async fn list_installed(&self) -> Result<Vec<Package>>;
}
