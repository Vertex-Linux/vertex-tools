use crate::managers::{
    aur::AurManager, flatpak::FlatpakManager, pacman::PacmanManager, vl::VlManager, Manager,
};
use crate::package::{Package, Source};
use anyhow::Result;

pub struct Resolver {
    pub pacman: PacmanManager,
    pub aur: AurManager,
    pub flatpak: FlatpakManager,
    pub vl: VlManager,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            pacman: PacmanManager::new(),
            aur: AurManager::new(),
            flatpak: FlatpakManager::new(),
            vl: VlManager::new(),
        }
    }

    pub async fn search_all(&self, query: &str) -> Result<Vec<Package>> {
        let (pm, aur, fp, vl) = tokio::join!(
            self.pacman.search(query),
            self.aur.search(query),
            self.flatpak.search(query),
            self.vl.search(query),
        );
        let mut all = Vec::new();
        for res in [pm, aur, fp, vl] {
            if let Ok(mut v) = res {
                all.append(&mut v);
            }
        }
        Ok(all)
    }

    pub async fn search_one(&self, source: &Source, query: &str) -> Result<Vec<Package>> {
        match source {
            Source::Pacman => self.pacman.search(query).await,
            Source::Aur => self.aur.search(query).await,
            Source::Flatpak => self.flatpak.search(query).await,
            Source::VertexLinux => self.vl.search(query).await,
        }
    }

    /// Find which sources carry an exact package name.
    /// VL uses a direct pkg.json lookup instead of a full search for efficiency.
    pub async fn find_sources(&self, name: &str) -> Vec<Source> {
        let (pm, aur, fp, vl) = tokio::join!(
            self.pacman.search(name),
            self.aur.search(name),
            self.flatpak.search(name),
            self.vl.fetch_pkg_json(name),
        );
        let mut sources = Vec::new();
        if pm.map(|v| v.iter().any(|p| p.name == name)).unwrap_or(false) {
            sources.push(Source::Pacman);
        }
        if aur.map(|v| v.iter().any(|p| p.name == name)).unwrap_or(false) {
            sources.push(Source::Aur);
        }
        if fp
            .map(|v| v.iter().any(|p| p.name == name || p.app_id.as_deref() == Some(name)))
            .unwrap_or(false)
        {
            sources.push(Source::Flatpak);
        }
        if vl.is_ok() {
            sources.push(Source::VertexLinux);
        }
        sources
    }

    /// Install VL packages, resolving pm/aur/fp dependencies first.
    pub async fn install_vl_packages(&self, packages: &[String]) -> Result<()> {
        for name in packages {
            let meta = self.vl.fetch_pkg_json(name).await?;

            if !meta.pm.is_empty() {
                println!(
                    "\x1b[36m==> Installing pacman deps for {}: {}\x1b[0m",
                    name,
                    meta.pm.join("  ")
                );
                self.pacman.install(&meta.pm).await?;
            }
            if !meta.aur.is_empty() {
                println!(
                    "\x1b[33m==> Installing AUR deps for {}: {}\x1b[0m",
                    name,
                    meta.aur.join("  ")
                );
                self.aur.install(&meta.aur).await?;
            }
            if !meta.fp.is_empty() {
                println!(
                    "\x1b[35m==> Installing Flatpak deps for {}: {}\x1b[0m",
                    name,
                    meta.fp.join("  ")
                );
                self.flatpak.install(&meta.fp).await?;
            }

            self.vl.install(&[name.clone()]).await?;
        }
        Ok(())
    }

    /// Auto-install: detect sources, prompt if ambiguous.
    pub async fn install_auto(&self, packages: &[String]) -> Result<()> {
        for name in packages {
            let sources = self.find_sources(name).await;
            let source = match sources.len() {
                0 => {
                    println!(
                        "\x1b[33m==> '{}' not found by exact name. Trying pacman…\x1b[0m",
                        name
                    );
                    Source::Pacman
                }
                1 => sources.into_iter().next().unwrap(),
                _ => {
                    println!("\x1b[36m==> '{}' found in multiple sources:\x1b[0m", name);
                    for (i, s) in sources.iter().enumerate() {
                        println!("    [{}] {}", i + 1, s.label());
                    }
                    print!("Select [1-{}]: ", sources.len());
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    let idx: usize = line.trim().parse().unwrap_or(1);
                    sources
                        .into_iter()
                        .nth(idx.saturating_sub(1))
                        .unwrap_or(Source::Pacman)
                }
            };
            match source {
                Source::Pacman => self.pacman.install(&[name.clone()]).await?,
                Source::Aur => self.aur.install(&[name.clone()]).await?,
                Source::Flatpak => self.flatpak.install(&[name.clone()]).await?,
                Source::VertexLinux => self.install_vl_packages(&[name.clone()]).await?,
            }
        }
        Ok(())
    }

    pub async fn remove_auto(&self, packages: &[String]) -> Result<()> {
        let pm_res = self.pacman.remove(packages).await;
        if pm_res.is_err() {
            self.flatpak.remove(packages).await?;
        }
        Ok(())
    }

    pub async fn update_all(&self) -> Result<()> {
        println!("\x1b[36m══ Updating Pacman packages ══\x1b[0m");
        self.pacman.update().await?;
        println!("\x1b[33m══ Updating AUR packages ══\x1b[0m");
        self.aur.update().await?;
        println!("\x1b[35m══ Updating Flatpak packages ══\x1b[0m");
        self.flatpak.update().await?;
        println!("\x1b[32m══ Vertex Linux repo packages ══\x1b[0m");
        self.vl.update().await?;
        Ok(())
    }

    pub async fn list_all(&self) -> Result<()> {
        let (pm, aur, fp) = tokio::join!(
            self.pacman.list_installed(),
            self.aur.list_installed(),
            self.flatpak.list_installed(),
        );
        if let Ok(pkgs) = pm {
            println!("\x1b[36m── Pacman ({}) ──\x1b[0m", pkgs.len());
            for p in &pkgs {
                println!("  {} {}", p.name, p.version);
            }
        }
        if let Ok(pkgs) = aur {
            println!("\x1b[33m── AUR ({}) ──\x1b[0m", pkgs.len());
            for p in &pkgs {
                println!("  {} {}", p.name, p.version);
            }
        }
        if let Ok(pkgs) = fp {
            println!("\x1b[35m── Flatpak ({}) ──\x1b[0m", pkgs.len());
            for p in &pkgs {
                println!("  {} {}", p.name, p.version);
            }
        }
        Ok(())
    }
}
