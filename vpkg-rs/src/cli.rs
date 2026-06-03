use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vpkg", about = "Vertex Package Manager — VL · AUR · Pacman · Flatpak", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Vertex Linux repo operations
    Vl {
        #[command(subcommand)]
        action: Action,
    },
    /// AUR package manager operations
    Aur {
        #[command(subcommand)]
        action: Action,
    },
    /// Pacman (official repos) operations
    Pm {
        #[command(subcommand)]
        action: Action,
    },
    /// Flatpak operations
    Fp {
        #[command(subcommand)]
        action: Action,
    },
    /// Install packages (auto-detect source)
    Install {
        packages: Vec<String>,
    },
    /// Remove packages (auto-detect source)
    Remove {
        packages: Vec<String>,
    },
    /// Update all package managers
    Update,
    /// Search packages across all sources (opens TUI)
    Search {
        query: Option<String>,
    },
    /// List installed packages from all sources
    List,
}

#[derive(Subcommand)]
pub enum Action {
    /// Install packages
    Install { packages: Vec<String> },
    /// Remove packages
    Remove { packages: Vec<String> },
    /// Update all packages from this source
    Update,
    /// Search packages (opens TUI filtered to this source)
    Search { query: Option<String> },
    /// List installed packages from this source
    List,
}
