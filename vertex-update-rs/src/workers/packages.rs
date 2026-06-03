use crate::workers::Msg;
use eframe::egui;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

/// Run a sequence of shell commands in a background thread, streaming each line
/// of stdout and stderr back through `tx` as `Msg::Log`. Sends `Msg::Done` when
/// all commands have finished.
pub fn run(tx: Sender<Msg>, ctx: egui::Context, commands: Vec<Vec<String>>) {
    std::thread::spawn(move || {
        let mut all_ok = true;

        for cmd in &commands {
            let _ = tx.send(Msg::Log(format!("\n$ {}", cmd.join(" "))));
            ctx.request_repaint();

            let result = Command::new(&cmd[0])
                .args(&cmd[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match result {
                Err(e) => {
                    let _ = tx.send(Msg::Log(format!("[error starting process: {e}]")));
                    ctx.request_repaint();
                    all_ok = false;
                }
                Ok(mut child) => {
                    let stdout = child.stdout.take().unwrap();
                    let stderr = child.stderr.take().unwrap();

                    // Read stderr on a separate thread so we don't deadlock
                    let tx2 = tx.clone();
                    let ctx2 = ctx.clone();
                    let h_err = std::thread::spawn(move || {
                        for line in BufReader::new(stderr).lines().flatten() {
                            let _ = tx2.send(Msg::Log(line));
                            ctx2.request_repaint();
                        }
                    });

                    for line in BufReader::new(stdout).lines().flatten() {
                        let _ = tx.send(Msg::Log(line));
                        ctx.request_repaint();
                    }
                    h_err.join().ok();

                    match child.wait() {
                        Ok(s) if !s.success() => {
                            let _ = tx.send(Msg::Log(format!(
                                "[exited with code {}]",
                                s.code().unwrap_or(-1)
                            )));
                            ctx.request_repaint();
                            all_ok = false;
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Log(format!("[wait error: {e}]")));
                            ctx.request_repaint();
                            all_ok = false;
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = tx.send(Msg::Done(all_ok));
        ctx.request_repaint();
    });
}
