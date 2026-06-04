use crate::constants::{DRIVERS_API, SELF_RELEASES_API, VPKG_API};
use crate::net::{fetch_bytes, fetch_json};
use crate::workers::Msg;
use eframe::egui;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

/// Download the selected tools from GitHub releases, then install them all
/// in one `pkexec bash -c "…"` call so the user only sees a single auth prompt.
pub fn run(
    tx: Sender<Msg>,
    ctx: egui::Context,
    do_drivers: bool,
    do_vpkg: bool,
    do_self: bool,
    do_calla: bool,
) {
    std::thread::spawn(move || {
        macro_rules! log {
            ($($t:tt)*) => {{
                let _ = tx.send(Msg::Log(format!($($t)*)));
                ctx.request_repaint();
            }};
        }

        let mut files: Vec<(String, String)> = Vec::new(); // (tmp_path, dest_path)
        let mut errors = 0usize;

        if do_drivers {
            log!("Fetching latest vertex-drivers release…");
            match fetch_drivers() {
                Ok(Some((src, dst, tag, kb))) => {
                    log!("  vertex-drivers {tag} ready  ({kb} KB)");
                    files.push((src, dst));
                }
                Ok(None) => {
                    log!("  [error] 'vertex-drivers' asset not found in latest release");
                    errors += 1;
                }
                Err(e) => {
                    log!("  [error] vertex-drivers: {e}");
                    errors += 1;
                }
            }
        }

        if do_vpkg {
            log!("Fetching latest vpkg release…");
            match fetch_vpkg() {
                Ok(Some((src, dst, tag, kb))) => {
                    log!("  vpkg {tag} ready  ({kb} KB)");
                    files.push((src, dst));
                }
                Ok(None) => {
                    log!("  [error] vpkg asset not found for this architecture");
                    errors += 1;
                }
                Err(e) => {
                    log!("  [error] vpkg: {e}");
                    errors += 1;
                }
            }
        }

        if do_self {
            log!("Fetching latest vertex-update release…");
            match fetch_self_update() {
                Ok(Some((src, dst, tag, kb))) => {
                    log!("  vertex-update {tag} ready  ({kb} KB)");
                    files.push((src, dst));
                }
                Ok(None) => {
                    log!("  [error] No release tagged with -vu found, or asset missing");
                    errors += 1;
                }
                Err(e) => {
                    log!("  [error] vertex-update: {e}");
                    errors += 1;
                }
            }
        }

        if files.is_empty() {
            log!("Nothing downloaded — cannot install.");
            let _ = tx.send(Msg::Done(false));
            ctx.request_repaint();
            return;
        }

        log!("\nInstalling with elevated privileges…");
        // rm -f the destination first so we don't hit "Text file busy" when
        // overwriting a binary that is currently running (this process).
        let cmd = files
            .iter()
            .map(|(src, dst)| {
                format!("rm -f '{}' && cp '{}' '{}' && chmod 755 '{}'", dst, src, dst, dst)
            })
            .collect::<Vec<_>>()
            .join(" && ");

        let mut child = Command::new("pkexec")
            .args(["bash", "-c", &cmd])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("pkexec not found");

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let tx2 = tx.clone();
        let ctx2 = ctx.clone();
        let h = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().flatten() {
                let _ = tx2.send(Msg::Log(line));
                ctx2.request_repaint();
            }
        });
        for line in BufReader::new(stdout).lines().flatten() {
            let _ = tx.send(Msg::Log(line));
            ctx.request_repaint();
        }
        h.join().ok();

        let status = child.wait().unwrap();
        for (src, _) in &files {
            let _ = std::fs::remove_file(src);
        }

        if status.success() {
            log!("\nAll tools installed successfully.");
            if files
                .iter()
                .any(|(_, dst)| dst == "/usr/local/bin/vertex-update")
            {
                let _ = tx.send(Msg::SelfUpdated);
                ctx.request_repaint();
            }
        } else {
            log!(
                "\n[install exited with code {}]",
                status.code().unwrap_or(-1)
            );
            errors += 1;
        }

        // ── Calla Desktop ─────────────────────────────────────────────────────
        if do_calla {
            log!("\nSyncing package database before Calla install…");
            // Refresh pacman's db first — otherwise pacman can't find packages
            // and makepkg -si will fail with "database file not found" errors.
            let sync = Command::new("pkexec")
                .args(["pacman", "-Sy", "--noconfirm"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            if let Ok(mut child) = sync {
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                let tx2 = tx.clone(); let ctx2 = ctx.clone();
                let h = std::thread::spawn(move || {
                    for l in BufReader::new(stderr).lines().flatten() {
                        let _ = tx2.send(Msg::Log(l)); ctx2.request_repaint();
                    }
                });
                for l in BufReader::new(stdout).lines().flatten() {
                    let _ = tx.send(Msg::Log(l)); ctx.request_repaint();
                }
                h.join().ok();
                child.wait().ok();
            }

            log!("Installing / updating Calla Desktop…");
            match Command::new("vpkg")
                .args(["vl", "install", "calla", "--no-deps"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Err(e) => {
                    log!("  [error] Could not run vpkg: {e}");
                    errors += 1;
                }
                Ok(mut child) => {
                    let stdout = child.stdout.take().unwrap();
                    let stderr = child.stderr.take().unwrap();
                    let tx2 = tx.clone();
                    let ctx2 = ctx.clone();
                    let h = std::thread::spawn(move || {
                        for line in BufReader::new(stderr).lines().flatten() {
                            let _ = tx2.send(Msg::Log(line));
                            ctx2.request_repaint();
                        }
                    });
                    for line in BufReader::new(stdout).lines().flatten() {
                        let _ = tx.send(Msg::Log(line));
                        ctx.request_repaint();
                    }
                    h.join().ok();
                    match child.wait() {
                        Ok(s) if !s.success() => {
                            log!("  [error] Calla update exited with code {}", s.code().unwrap_or(-1));
                            errors += 1;
                        }
                        Err(e) => {
                            log!("  [error] {e}");
                            errors += 1;
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = tx.send(Msg::Done(errors == 0));
        ctx.request_repaint();
    });
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn write_tmp(prefix: &str, bytes: &[u8]) -> anyhow::Result<String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}{ts}"));
    let mut f = std::fs::File::create(&path)?;
    f.write_all(bytes)?;
    Ok(path.to_string_lossy().to_string())
}

fn fetch_drivers() -> anyhow::Result<Option<(String, String, String, usize)>> {
    let data = fetch_json(DRIVERS_API)?;
    let tag = data["tag_name"].as_str().unwrap_or("?").to_string();
    let url = data["assets"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|a| a["name"].as_str() == Some("vertex-drivers"))
                .and_then(|a| a["browser_download_url"].as_str())
        })
        .map(str::to_string);

    match url {
        None => Ok(None),
        Some(url) => {
            let bytes = fetch_bytes(&url)?;
            let kb = bytes.len() / 1024;
            let tmp = write_tmp("vertex-drivers-", &bytes)?;
            Ok(Some((tmp, "/usr/local/bin/vertex-drivers".into(), tag, kb)))
        }
    }
}

fn fetch_vpkg() -> anyhow::Result<Option<(String, String, String, usize)>> {
    let data = fetch_json(VPKG_API)?;
    let tag = data["tag_name"].as_str().unwrap_or("?").to_string();
    let arch = std::env::consts::ARCH; // "x86_64" or "aarch64"
    let asset_name = format!("vpkg-{arch}-unknown-linux-gnu");

    let url = data["assets"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|a| a["name"].as_str() == Some(asset_name.as_str()))
                .and_then(|a| a["browser_download_url"].as_str())
        })
        .map(str::to_string);

    match url {
        None => Ok(None),
        Some(url) => {
            let bytes = fetch_bytes(&url)?;
            let kb = bytes.len() / 1024;
            let tmp = write_tmp("vertex-vpkg-", &bytes)?;
            Ok(Some((tmp, "/usr/local/bin/vpkg".into(), tag, kb)))
        }
    }
}

fn fetch_self_update() -> anyhow::Result<Option<(String, String, String, usize)>> {
    let releases = fetch_json(SELF_RELEASES_API)?;
    let release = releases
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|r| r["tag_name"].as_str().map(|t| t.ends_with("-vu")).unwrap_or(false))
        })
        .cloned();

    let release = match release {
        Some(r) => r,
        None => return Ok(None),
    };

    let tag = release["tag_name"].as_str().unwrap_or("?").to_string();
    let arch = std::env::consts::ARCH;
    let asset_name = format!("vertex-update-{arch}-unknown-linux-gnu");

    let url = release["assets"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|a| a["name"].as_str() == Some(asset_name.as_str()))
                .and_then(|a| a["browser_download_url"].as_str())
        })
        .map(str::to_string);

    match url {
        None => Ok(None),
        Some(url) => {
            let bytes = fetch_bytes(&url)?;
            let kb = bytes.len() / 1024;
            let tmp = write_tmp("vertex-update-", &bytes)?;
            Ok(Some((tmp, "/usr/local/bin/vertex-update".into(), tag, kb)))
        }
    }
}
