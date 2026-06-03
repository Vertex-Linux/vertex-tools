use crate::constants::{OS_API_URL, OS_UPDATE_URL};
use crate::net::{fetch_bytes, fetch_json};
use crate::workers::{Msg, OsInfo};
use eframe::egui;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc::Sender;

/// Fetch the latest commit from the OS repo and report back the SHA + message.
pub fn check(tx: Sender<Msg>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let result = do_check();
        match result {
            Ok(info) => {
                let _ = tx.send(Msg::OsInfo(info));
            }
            Err(e) => {
                let _ = tx.send(Msg::OsError(e.to_string()));
            }
        }
        ctx.request_repaint();
    });
}

fn do_check() -> anyhow::Result<OsInfo> {
    let data = fetch_json(OS_API_URL)?;
    let sha = data["sha"].as_str().unwrap_or("").to_string();
    Ok(OsInfo {
        short: sha.chars().take(8).collect(),
        message: data["commit"]["message"]
            .as_str()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
        date: data["commit"]["author"]["date"]
            .as_str()
            .unwrap_or("")
            .get(..10)
            .unwrap_or("")
            .to_string(),
        sha,
    })
}

/// Download update.sh into a temp file and report the path back.
pub fn fetch_script(tx: Sender<Msg>, ctx: egui::Context, sha: String) {
    std::thread::spawn(move || {
        match do_fetch() {
            Ok(path) => {
                let _ = tx.send(Msg::ScriptReady { path, sha });
            }
            Err(e) => {
                let _ = tx.send(Msg::ScriptError(e.to_string()));
            }
        }
        ctx.request_repaint();
    });
}

fn do_fetch() -> anyhow::Result<String> {
    let bytes = fetch_bytes(OS_UPDATE_URL)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vertex-os-update-{ts}.sh"));
    let mut f = std::fs::File::create(&path)?;
    f.write_all(&bytes)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(path.to_string_lossy().to_string())
}
