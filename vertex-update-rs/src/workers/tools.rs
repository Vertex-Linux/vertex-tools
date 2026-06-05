use crate::constants::{DRIVERS_API, SELF_RELEASES_API, VPKG_RELEASES_API};
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
                    log!("  vpkg: no release published yet — skipping");
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

        if !files.is_empty() {
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
        } // end !files.is_empty()

        // ── Calla Desktop ─────────────────────────────────────────────────────
        if do_calla {
            log!("\nUpdating Calla Desktop…");

            // Clean any previous build dir
            let _ = std::fs::remove_dir_all("/tmp/calla-upd");

            // 1. git clone
            log!("  Cloning Vertex-Linux/Calla…");
            let ok = run_streaming(
                "git",
                &["clone", "https://github.com/Vertex-Linux/Calla.git", "/tmp/calla-upd/"],
                None,
                &tx, &ctx,
            );
            if !ok {
                log!("  [error] git clone failed");
                errors += 1;
            } else {
                // 2. pkexec makepkg -sl  (build + sync deps, no install step)
                log!("  Building package…");
                let ok = run_streaming(
                    "makepkg",
                    &["-s"],
                    Some("/tmp/calla-upd"),
                    &tx, &ctx,
                );
                if !ok {
                    log!("  [error] makepkg failed");
                    errors += 1;
                } else {
                    // 3. find the built .pkg.tar.zst and install it
                    match find_built_pkg("/tmp/calla-upd") {
                        Err(e) => {
                            log!("  [error] {e}");
                            errors += 1;
                        }
                        Ok(pkg) => {
                            log!("  Installing {}…", pkg);
                            let ok = run_streaming(
                                "pkexec",
                                &["pacman", "-U", "--noconfirm", &pkg],
                                Some("/tmp/calla-upd"),
                                &tx, &ctx,
                            );
                            if !ok {
                                log!("  [error] pacman -U failed");
                                errors += 1;
                            }
                        }
                    }
                }
            }
        }

        let _ = tx.send(Msg::Done(errors == 0));
        ctx.request_repaint();
    });
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Run a command, stream stdout+stderr to the log, return true if it succeeded.
fn run_streaming(
    program: &str,
    args: &[&str],
    cwd: Option<&str>,
    tx: &Sender<Msg>,
    ctx: &egui::Context,
) -> bool {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Err(e) => {
            let _ = tx.send(Msg::Log(format!("[error starting '{}': {}]", program, e)));
            ctx.request_repaint();
            false
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
            matches!(child.wait(), Ok(s) if s.success())
        }
    }
}

/// Find the first .pkg.tar.zst / .pkg.tar.xz in a directory.
fn find_built_pkg(dir: &str) -> anyhow::Result<String> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".pkg.tar.zst") || name.ends_with(".pkg.tar.xz") {
            return Ok(path.to_string_lossy().to_string());
        }
    }
    anyhow::bail!("No .pkg.tar.zst found in {}", dir)
}

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
    let releases = fetch_json(VPKG_RELEASES_API)?;
    let arch = std::env::consts::ARCH; // "x86_64" or "aarch64"
    let asset_name = format!("vpkg-{arch}-unknown-linux-gnu");

    // Find the most recent release whose tag starts with "vpkg-".
    // Using /releases/latest would pick up -vu releases which have no vpkg assets.
    let release = releases
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|r| {
                r["tag_name"]
                    .as_str()
                    .map(|t| t.starts_with("vpkg-"))
                    .unwrap_or(false)
            })
        })
        .cloned();

    let release = match release {
        Some(r) => r,
        None => return Ok(None), // no vpkg release published yet — caller logs informational
    };

    let tag = release["tag_name"].as_str().unwrap_or("?").to_string();
    let url = release["assets"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|a| a["name"].as_str() == Some(asset_name.as_str()))
                .and_then(|a| a["browser_download_url"].as_str())
        })
        .map(str::to_string);

    match url {
        // Release exists but no matching asset — surface as an error so the user notices
        None => anyhow::bail!("release {tag} has no asset '{asset_name}'"),
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
