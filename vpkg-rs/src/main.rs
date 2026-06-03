mod cli;
mod managers;
mod package;
mod resolver;
mod tui;

use anyhow::Result;
use cli::{Action, Cli, Commands};
use clap::Parser;
use managers::Manager;
use package::Source;
use resolver::Resolver;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let resolver = Arc::new(Resolver::new());

    match cli.command {
        Some(Commands::Vl { action }) => {
            run_action(&resolver.vl, action, Some(Source::VertexLinux), &resolver).await?;
        }
        Some(Commands::Aur { action }) => {
            run_action(&resolver.aur, action, Some(Source::Aur), &resolver).await?;
        }
        Some(Commands::Pm { action }) => {
            run_action(&resolver.pacman, action, Some(Source::Pacman), &resolver).await?;
        }
        Some(Commands::Fp { action }) => {
            run_action(&resolver.flatpak, action, Some(Source::Flatpak), &resolver).await?;
        }
        Some(Commands::Install { packages }) => {
            resolver.install_auto(&packages).await?;
        }
        Some(Commands::Remove { packages }) => {
            resolver.remove_auto(&packages).await?;
        }
        Some(Commands::Update) => {
            resolver.update_all().await?;
        }
        Some(Commands::Search { query }) => {
            run_search_tui(resolver, query, None).await?;
        }
        Some(Commands::List) => {
            resolver.list_all().await?;
        }
        None => {
            // No args → open interactive TUI
            run_search_tui(resolver, None, None).await?;
        }
    }

    Ok(())
}

async fn run_action<M: Manager>(
    manager: &M,
    action: Action,
    filter: Option<Source>,
    resolver: &Arc<Resolver>,
) -> Result<()> {
    match action {
        Action::Install { packages } => {
            manager.install(&packages).await?;
        }
        Action::Remove { packages } => {
            manager.remove(&packages).await?;
        }
        Action::Update => {
            manager.update().await?;
        }
        Action::Search { query } => {
            run_search_tui(Arc::clone(resolver), query, filter).await?;
        }
        Action::List => {
            for pkg in manager.list_installed().await? {
                println!("{} {}", pkg.name, pkg.version);
            }
        }
    }
    Ok(())
}

async fn run_search_tui(
    resolver: Arc<Resolver>,
    query: Option<String>,
    filter: Option<Source>,
) -> Result<()> {
    loop {
        let packages = tui::run(Arc::clone(&resolver), query.clone(), filter.clone()).await?;

        match packages {
            None => break, // user quit
            Some(pkgs) => {
                // Group packages by source and install
                let mut by_source: std::collections::HashMap<String, Vec<String>> =
                    std::collections::HashMap::new();
                for pkg in &pkgs {
                    by_source
                        .entry(pkg.source.short().to_string())
                        .or_default()
                        .push(pkg.install_name().to_string());
                }

                println!();
                for pkg in &pkgs {
                    eprintln!(
                        "\x1b[36m==> Installing {} ({}) via {}\x1b[0m",
                        pkg.name,
                        pkg.version,
                        pkg.source.label()
                    );
                }

                for (source_key, names) in &by_source {
                    let res = match source_key.as_str() {
                        "vl" => resolver.vl.install(names).await,
                        "aur" => resolver.aur.install(names).await,
                        "pm" => resolver.pacman.install(names).await,
                        "fp" => resolver.flatpak.install(names).await,
                        _ => Ok(()),
                    };
                    if let Err(e) = res {
                        eprintln!("\x1b[31m==> Error: {}\x1b[0m", e);
                    }
                }

                // Ask if the user wants to search again
                eprint!("\n\x1b[36m==> Return to search? [Y/n] \x1b[0m");
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if line.trim().eq_ignore_ascii_case("n") {
                    break;
                }
            }
        }
    }
    Ok(())
}
