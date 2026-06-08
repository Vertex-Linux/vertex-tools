use crate::colors;
use crate::workers::{Msg, OsInfo};
use eframe::egui;
use std::sync::mpsc;

// ── State types ───────────────────────────────────────────────────────────────

#[derive(Default, PartialEq)]
pub enum OsState {
    #[default]
    NotChecked,
    Checking,
    UpToDate,
    UpdateAvailable,
    Error(String),
}

#[derive(Default, PartialEq)]
pub enum CurrentOp {
    #[default]
    None,
    Packages,
    Tools,
    OsScript,
}

pub enum StatusLevel {
    Info,
    Ok,
    Warn,
    Error,
}

impl StatusLevel {
    pub fn color(&self) -> egui::Color32 {
        match self {
            StatusLevel::Info => colors::TEXT_DIM,
            StatusLevel::Ok => colors::OK,
            StatusLevel::Warn => colors::WARN,
            StatusLevel::Error => colors::ERR,
        }
    }
}

// ── App struct ────────────────────────────────────────────────────────────────

pub struct App {
    // worker communication
    pub tx: mpsc::Sender<Msg>,
    pub rx: mpsc::Receiver<Msg>,

    // package update checkboxes
    pub chk_pacman: bool,
    pub chk_aur: bool,
    pub chk_flatpak: bool,

    // tool update checkboxes
    pub chk_drivers: bool,
    pub chk_vpkg: bool,
    pub chk_self: bool,
    pub chk_calla: bool,
    pub chk_vterm: bool,
    pub chk_vstore: bool,

    // OS update state
    pub os_installed: String,
    pub os_info: Option<OsInfo>,
    pub os_state: OsState,

    // Filled when ScriptReady arrives; executed next frame so we have ctx
    pub pending_os_cmds: Option<Vec<Vec<String>>>,
    pub pending_sha: String,

    // log
    pub log: String,

    // status bar
    pub status: String,
    pub status_level: StatusLevel,

    pub busy: bool,
    pub current_op: CurrentOp,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            chk_pacman: true,
            chk_aur: true,
            chk_flatpak: true,
            chk_drivers: true,
            chk_vpkg: true,
            chk_self: true,
            chk_calla: true,
            chk_vterm: true,
            chk_vstore: true,
            os_installed: read_version(),
            os_info: None,
            os_state: OsState::NotChecked,
            pending_os_cmds: None,
            pending_sha: String::new(),
            log: String::new(),
            status: "Ready.".into(),
            status_level: StatusLevel::Info,
            busy: false,
            current_op: CurrentOp::None,
        }
    }

    // ── Message pump ──────────────────────────────────────────────────────────

    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Log(line) => {
                    self.log.push_str(&line);
                    self.log.push('\n');
                }

                Msg::Done(ok) => {
                    self.busy = false;
                    match self.current_op {
                        CurrentOp::Packages => {
                            if ok {
                                self.set_status("Package updates complete.", StatusLevel::Ok);
                            } else {
                                self.set_status(
                                    "Finished with errors — check log.",
                                    StatusLevel::Warn,
                                );
                            }
                        }
                        CurrentOp::Tools => {
                            if ok {
                                self.set_status("Tool updates applied.", StatusLevel::Ok);
                            } else {
                                self.set_status(
                                    "Tool update finished with errors — check log.",
                                    StatusLevel::Warn,
                                );
                            }
                        }
                        CurrentOp::OsScript => {
                            if ok {
                                self.os_installed = self.pending_sha.clone();
                                self.os_state = OsState::UpToDate;
                                self.set_status(
                                    "OS update applied successfully!",
                                    StatusLevel::Ok,
                                );
                            } else {
                                self.set_status(
                                    "OS update finished with errors.",
                                    StatusLevel::Warn,
                                );
                            }
                        }
                        CurrentOp::None => {}
                    }
                    self.current_op = CurrentOp::None;
                }

                Msg::OsInfo(info) => {
                    self.busy = false;
                    let up_to_date =
                        self.os_installed != "unknown" && self.os_installed == info.sha;
                    if up_to_date {
                        self.os_state = OsState::UpToDate;
                        self.set_status("OS is up to date.", StatusLevel::Ok);
                    } else {
                        self.os_state = OsState::UpdateAvailable;
                        self.set_status("OS update available!", StatusLevel::Ok);
                    }
                    self.os_info = Some(info);
                }

                Msg::OsError(e) => {
                    self.busy = false;
                    self.os_state = OsState::Error(e.clone());
                    self.set_status(&format!("Check failed: {e}"), StatusLevel::Error);
                }

                Msg::ScriptReady { path, sha } => {
                    self.pending_sha = sha.clone();
                    self.log.push_str("Script downloaded. Running with elevated privileges…\n");
                    // Queue the commands; they'll be launched next frame so we have ctx
                    self.pending_os_cmds = Some(vec![
                        vec!["pkexec".into(), "bash".into(), path],
                        vec![
                            "pkexec".into(),
                            "bash".into(),
                            "-c".into(),
                            format!("echo '{}' > /etc/vertex-release", sha),
                        ],
                    ]);
                }

                Msg::ScriptError(e) => {
                    self.busy = false;
                    self.set_status(&format!("Download failed: {e}"), StatusLevel::Error);
                }

                Msg::SelfUpdated => {
                    self.log.push_str(
                        "\n  Vertex Updater was replaced on disk.\n  Close and relaunch this app to run the new version.\n",
                    );
                }
            }
        }
    }

    // ── Action starters ───────────────────────────────────────────────────────

    pub fn start_pkg_updates(&mut self, ctx: &egui::Context) {
        let mut cmds: Vec<Vec<String>> = Vec::new();
        if self.chk_pacman {
            cmds.push(vec!["pkexec".into(), "vpkg".into(), "pm".into(), "update".into()]);
        }
        if self.chk_aur {
            cmds.push(vec!["vpkg".into(), "aur".into(), "update".into()]);
        }
        if self.chk_flatpak {
            cmds.push(vec!["pkexec".into(), "vpkg".into(), "fp".into(), "update".into()]);
        }
        if cmds.is_empty() {
            self.log.push_str("Nothing selected.\n");
            return;
        }
        self.busy = true;
        self.current_op = CurrentOp::Packages;
        self.log.push_str("\n=== Package Updates ===\n");
        self.set_status("Updating packages…", StatusLevel::Info);
        crate::workers::packages::run(self.tx.clone(), ctx.clone(), cmds);
    }

    pub fn start_tool_updates(&mut self, ctx: &egui::Context) {
        if !self.chk_drivers && !self.chk_vpkg && !self.chk_self && !self.chk_calla && !self.chk_vterm && !self.chk_vstore {
            self.log.push_str("No tools selected.\n");
            return;
        }
        self.busy = true;
        self.current_op = CurrentOp::Tools;
        self.log.push_str("\n=== Vertex Tool Updates ===\n");
        self.set_status("Downloading tool updates…", StatusLevel::Info);
        crate::workers::tools::run(
            self.tx.clone(),
            ctx.clone(),
            self.chk_drivers,
            self.chk_vpkg,
            self.chk_self,
            self.chk_calla,
            self.chk_vterm,
            self.chk_vstore,
        );
    }

    pub fn start_os_check(&mut self, ctx: &egui::Context) {
        self.busy = true;
        self.os_state = OsState::Checking;
        self.set_status("Checking for OS updates…", StatusLevel::Info);
        crate::workers::os::check(self.tx.clone(), ctx.clone());
    }

    pub fn start_os_update(&mut self, ctx: &egui::Context) {
        if let Some(info) = &self.os_info {
            let sha = info.sha.clone();
            self.busy = true;
            self.current_op = CurrentOp::OsScript;
            self.log.push_str("=== Downloading OS update script…\n");
            crate::workers::os::fetch_script(self.tx.clone(), ctx.clone(), sha);
        }
    }

    fn set_status(&mut self, text: &str, level: StatusLevel) {
        self.status = text.to_string();
        self.status_level = level;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        // Fire pending OS update commands now that we have `ctx`
        if let Some(cmds) = self.pending_os_cmds.take() {
            crate::workers::packages::run(self.tx.clone(), ctx.clone(), cmds);
        }

        if self.busy {
            ctx.request_repaint();
        }

        crate::ui::render(ctx, self);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_version() -> String {
    std::fs::read_to_string(crate::constants::VERSION_FILE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn apply_style(ctx: &egui::Context) {
    let mut vis = egui::Visuals::dark();
    vis.panel_fill = colors::BG;
    vis.window_fill = colors::BG;
    vis.extreme_bg_color = colors::BG_DARK;
    vis.override_text_color = Some(colors::TEXT);

    vis.widgets.noninteractive.bg_fill = colors::BG;
    vis.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, colors::BORDER);

    vis.widgets.inactive.bg_fill = colors::BTN;
    vis.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    vis.widgets.hovered.bg_fill = colors::BTN_HOVER;
    vis.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    vis.widgets.active.bg_fill = colors::BTN_PRESSED;

    vis.selection.bg_fill = colors::BTN;

    ctx.set_visuals(vis);

    let mut style = (*ctx.style()).clone();
    use egui::{FontFamily::Proportional, FontId, TextStyle::*};
    style.text_styles.insert(Heading, FontId::new(22.0, Proportional));
    style.text_styles.insert(Body, FontId::new(13.0, Proportional));
    style.text_styles.insert(Button, FontId::new(13.0, Proportional));
    style.spacing.button_padding = egui::vec2(16.0, 6.0);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}
