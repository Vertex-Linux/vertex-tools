use crate::app::{App, OsState};
use crate::colors;
use eframe::egui::{self, Color32, RichText, Stroke, Ui};

pub fn render(ctx: &egui::Context, app: &mut App) {
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                render_header(ui);
                ui.add_space(4.0);
                ui.add(egui::Separator::default().spacing(8.0));
                ui.add_space(4.0);

                render_packages(ui, app, ctx);
                ui.add_space(10.0);
                render_tools(ui, app, ctx);
                ui.add_space(10.0);
                render_os(ui, app, ctx);
                ui.add_space(10.0);
                render_log(ui, app);
                ui.add_space(4.0);
                render_status(ui, app);
            });
    });
}

// ── Sections ──────────────────────────────────────────────────────────────────

fn render_header(ui: &mut Ui) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Vertex Updater")
                .color(colors::ACCENT)
                .strong()
                .size(22.0),
        );
        ui.add_space(14.0);
        ui.label(
            RichText::new("Keep your Vertex Linux system up to date")
                .color(colors::TEXT_DIM)
                .size(12.0),
        );
    });
}

fn render_packages(ui: &mut Ui, app: &mut App, ctx: &egui::Context) {
    section(ui, "Package Updates", |ui| {
        ui.checkbox(
            &mut app.chk_pacman,
            RichText::new("System packages  (vpkg pm update)").color(colors::TEXT_SEC),
        );
        ui.checkbox(
            &mut app.chk_aur,
            RichText::new("AUR packages  (vpkg aur update — git + makepkg)")
                .color(colors::TEXT_SEC),
        );
        ui.checkbox(
            &mut app.chk_flatpak,
            RichText::new("Flatpak applications  (vpkg fp update)").color(colors::TEXT_SEC),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!app.busy, |ui| {
                if action_btn(ui, "Update Packages").clicked() {
                    app.start_pkg_updates(ctx);
                }
            });
        });
    });
}

fn render_tools(ui: &mut Ui, app: &mut App, ctx: &egui::Context) {
    section(ui, "Vertex Tool Updates  (from GitHub releases)", |ui| {
        ui.checkbox(
            &mut app.chk_drivers,
            RichText::new(
                "Vertex Driver Manager  (Vertex-Linux/vertex-graphics-installer — latest release)",
            )
            .color(colors::TEXT_SEC),
        );
        ui.checkbox(
            &mut app.chk_vpkg,
            RichText::new("vpkg package manager  (arc360alt/vertex-tools — latest release)")
                .color(colors::TEXT_SEC),
        );
        ui.checkbox(
            &mut app.chk_self,
            RichText::new(
                "Vertex Updater itself  (arc360alt/vertex-tools — latest release tagged -vu)",
            )
            .color(colors::TEXT_SEC),
        );
        ui.checkbox(
            &mut app.chk_calla,
            RichText::new("Calla Desktop  (vpkg vl install calla --no-deps)")
                .color(colors::TEXT_SEC),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!app.busy, |ui| {
                if action_btn(ui, "Update Tools").clicked() {
                    app.start_tool_updates(ctx);
                }
            });
        });
    });
}

fn render_os(ui: &mut Ui, app: &mut App, ctx: &egui::Context) {
    section(ui, "OS Update  (syncs system config from GitHub commits)", |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Installed:").color(colors::TEXT_DIM));
            ui.label(
                RichText::new(&app.os_installed)
                    .monospace()
                    .color(colors::TEXT_DIM),
            );
            ui.add_space(20.0);
            ui.label(RichText::new("Latest:").color(colors::TEXT_DIM));

            match &app.os_state {
                OsState::NotChecked => {
                    ui.label(
                        RichText::new("(not checked)").color(colors::TEXT_DIM),
                    );
                }
                OsState::Checking => {
                    ui.spinner();
                    ui.label(RichText::new("checking…").color(colors::TEXT_DIM));
                }
                OsState::UpToDate => {
                    if let Some(info) = &app.os_info {
                        ui.label(
                            RichText::new(format!("{}  ({})", info.short, info.date))
                                .monospace()
                                .color(colors::TEXT_DIM),
                        );
                    }
                }
                OsState::UpdateAvailable => {
                    if let Some(info) = &app.os_info {
                        ui.label(
                            RichText::new(format!("{}  ({})", info.short, info.date))
                                .monospace()
                                .color(colors::OK),
                        );
                    }
                }
                OsState::Error(_) => {
                    ui.label(RichText::new("error").color(colors::ERR));
                }
            }
        });

        if let Some(info) = &app.os_info {
            if !info.message.is_empty() {
                ui.label(
                    RichText::new(&info.message)
                        .color(colors::TEXT_DIM)
                        .italics(),
                );
            }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!app.busy, |ui| {
                let check_btn = egui::Button::new(
                    RichText::new("Check for Updates").color(colors::TEXT_SEC),
                )
                .fill(Color32::from_rgb(30, 13, 60))
                .stroke(Stroke::new(1.0, colors::BTN));

                if ui.add(check_btn).clicked() {
                    app.start_os_check(ctx);
                }
            });

            ui.add_space(4.0);

            let can_apply = matches!(app.os_state, OsState::UpdateAvailable) && !app.busy;
            ui.add_enabled_ui(can_apply, |ui| {
                if action_btn(ui, "Apply OS Update").clicked() {
                    app.start_os_update(ctx);
                }
            });
        });
    });
}

fn render_log(ui: &mut Ui, app: &mut App) {
    ui.label(
        RichText::new("Output Log")
            .color(colors::TEXT_DIM)
            .size(11.0)
            .strong(),
    );

    let log_frame = egui::Frame::default()
        .fill(colors::BG_DARK)
        .stroke(Stroke::new(1.0, colors::BORDER_SEC))
        .rounding(4.0)
        .inner_margin(4.0);

    log_frame.show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_source("log_area")
            .stick_to_bottom(true)
            .min_scrolled_height(160.0)
            .max_height(260.0)
            .show(ui, |ui| {
                ui.add_sized(
                    [ui.available_width(), 0.0],
                    egui::TextEdit::multiline(&mut app.log)
                        .font(egui::TextStyle::Monospace)
                        .text_color(colors::ACCENT)
                        .desired_rows(8)
                        .interactive(false),
                );
            });
    });
}

fn render_status(ui: &mut Ui, app: &App) {
    ui.label(RichText::new(&app.status).color(app.status_level.color()).size(12.0));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn section(ui: &mut Ui, title: &str, f: impl FnOnce(&mut Ui)) {
    let frame = egui::Frame::default()
        .fill(colors::BG)
        .stroke(Stroke::new(1.0, colors::BORDER))
        .rounding(8.0)
        .inner_margin(12.0);

    frame.show(ui, |ui| {
        ui.label(RichText::new(title).color(colors::ACCENT).strong().size(13.0));
        ui.add_space(8.0);
        f(ui);
    });
}

fn action_btn(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
            .fill(colors::BTN)
            .min_size(egui::vec2(140.0, 30.0)),
    )
}
