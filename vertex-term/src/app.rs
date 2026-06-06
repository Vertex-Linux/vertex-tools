use crate::{
    blur,
    config::Config,
    pty::Pty,
    terminal::Terminal,
    theme::{self, Theme},
};
use egui::{
    text::LayoutJob, Color32, FontFamily, FontId, Key, Margin, Modifiers, ScrollArea, TextFormat,
    Vec2,
};
use vte::Parser;

pub struct VertexTerm {
    config: Config,
    theme: Theme,
    terminal: Terminal,
    pty: Pty,
    parser: Parser,
    show_settings: bool,
    cursor_visible: bool,
    last_blink: std::time::Instant,
    shell_input: String,
    user_themes: Vec<Theme>,
    /// False until the first update() so blur is applied after the window is mapped.
    blur_applied: bool,
}

impl VertexTerm {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Create ~/.vertex-term/themes/ and seed example files on first run.
        theme::init_user_theme_dir();

        let config = Config::load();
        let user_themes = theme::load_user_themes();
        let active_theme = theme::by_name(&config.theme);
        let pty = Pty::spawn(&config.shell, 80, 24).expect("Failed to spawn PTY");
        let terminal = Terminal::new(80, 24, config.scrollback_lines);
        let shell_input = config.shell.clone();
        Self {
            config,
            theme: active_theme,
            terminal,
            pty,
            parser: Parser::new(),
            show_settings: false,
            cursor_visible: true,
            last_blink: std::time::Instant::now(),
            shell_input,
            user_themes,
            blur_applied: false,
        }
    }

    fn process_pty_output(&mut self) {
        let bytes = self.pty.drain();
        for byte in bytes {
            self.parser.advance(&mut self.terminal, byte);
        }
    }

    // Returns (cell_width, cell_height) — no item spacing included, we zero it.
    fn cell_size(font_size: f32) -> Vec2 {
        Vec2::new(font_size * 0.601, font_size * 1.2)
    }

    fn apply_visuals(&self, ctx: &egui::Context) {
        let [r, g, b] = self.theme.background;
        let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
        let mut vis = if luma > 140.0 { egui::Visuals::light() } else { egui::Visuals::dark() };

        let bg = self.theme.bg_with_alpha(self.config.opacity);
        let fg = self.theme.fg();
        vis.panel_fill = bg;
        vis.window_fill = bg;
        vis.override_text_color = Some(fg);

        // Widget colors that match the theme
        let accent = self.theme.ansi_color(4); // blue
        vis.selection.bg_fill = self.theme.selection_color();
        vis.widgets.active.bg_fill = accent;
        vis.widgets.hovered.bg_fill = dim(accent, 0.7);
        vis.widgets.inactive.bg_fill = dim(bg, 0.8);
        vis.widgets.noninteractive.bg_fill = bg;
        vis.widgets.noninteractive.fg_stroke.color = fg;
        vis.widgets.inactive.fg_stroke.color = fg;
        vis.widgets.hovered.fg_stroke.color = fg;
        vis.widgets.active.fg_stroke.color = fg;

        ctx.set_visuals(vis);
    }

    fn render_terminal(&mut self, ui: &mut egui::Ui) {
        // Zero row spacing so virtual height matches actual rendered height exactly.
        ui.spacing_mut().item_spacing.y = 0.0;

        let font_size = self.config.font_size;
        let cell_sz = Self::cell_size(font_size);

        // Resize terminal to fill available space before creating the scroll area,
        // so show_rows gets the correct row count on the same frame.
        let avail = ui.available_size();
        let new_cols = ((avail.x / cell_sz.x).floor() as usize).max(4);
        let new_rows = ((avail.y / cell_sz.y).floor() as usize).max(2);
        if new_cols != self.terminal.cols || new_rows != self.terminal.rows {
            self.terminal.resize(new_cols, new_rows);
            self.pty.resize(new_cols as u16, new_rows as u16);
        }

        let total_lines = self.terminal.scrollback.len() + self.terminal.rows;
        let font_id = FontId::new(font_size, FontFamily::Monospace);
        let theme = self.theme.clone();
        let cursor_visible = self.cursor_visible;
        let opacity = self.config.opacity;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.terminal.scroll_offset == 0)
            .show_rows(ui, cell_sz.y, total_lines, |ui, row_range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let scrollback_len = self.terminal.scrollback.len();
                for row_idx in row_range {
                    let row: &Vec<crate::terminal::Cell> = if row_idx < scrollback_len {
                        &self.terminal.scrollback[row_idx]
                    } else {
                        &self.terminal.grid[row_idx - scrollback_len]
                    };
                    let is_active_row = row_idx >= scrollback_len
                        && (row_idx - scrollback_len) == self.terminal.cursor_row;

                    // Batch consecutive cells that share the same visual style into one
                    // LayoutJob segment instead of one segment per character (80→ typically
                    // 3-10 segments per row), which cuts text layout cost ~10-20×.
                    let mut job = LayoutJob::default();
                    let mut col = 0usize;
                    while col < row.len() {
                        let (fg0, bg0, it0, ul0) = cell_style(&row[col], col, is_active_row, self.terminal.cursor_col, cursor_visible, &theme, opacity);
                        let mut run = String::new();
                        while col < row.len() {
                            let (fg, bg, it, ul) = cell_style(&row[col], col, is_active_row, self.terminal.cursor_col, cursor_visible, &theme, opacity);
                            if fg != fg0 || bg != bg0 || it != it0 || ul != ul0 { break; }
                            run.push(if row[col].ch == '\0' { ' ' } else { row[col].ch });
                            col += 1;
                        }
                        job.append(
                            &run,
                            0.0,
                            TextFormat {
                                font_id: font_id.clone(),
                                color: fg0,
                                background: bg0,
                                italics: it0,
                                underline: if ul0 { egui::Stroke::new(1.0, fg0) } else { egui::Stroke::NONE },
                                ..Default::default()
                            },
                        );
                    }
                    ui.label(job);
                }
            });
    }

    fn settings_panel(&mut self, ctx: &egui::Context) -> bool {
        let mut restart = false;
        let bg = self.theme.bg_with_alpha(self.config.opacity);
        let fg = self.theme.fg();
        egui::Window::new("⚙ Settings")
            .resizable(false)
            .collapsible(false)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(bg)
                    .stroke(egui::Stroke::new(1.0, dim(fg, 0.3)))
                    .inner_margin(Margin::same(12.0)),
            )
            .show(ctx, |ui| {
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Theme");
                        ui.vertical(|ui| {
                            let all = theme::all_themes(&self.user_themes);
                            egui::ComboBox::from_id_salt("theme_picker")
                                .selected_text(&self.config.theme)
                                .show_ui(ui, |ui| {
                                    let builtin_count = theme::builtin_themes().len();
                                    for (i, t) in all.iter().enumerate() {
                                        if i == builtin_count && !self.user_themes.is_empty() {
                                            ui.separator();
                                        }
                                        if ui.selectable_label(self.config.theme == t.name, &t.name).clicked() {
                                            self.config.theme = t.name.clone();
                                            self.theme = t.clone();
                                        }
                                    }
                                });
                            if !self.user_themes.is_empty() {
                                ui.label(
                                    egui::RichText::new(format!("{} user theme(s) loaded from ~/.vertex-term/themes/", self.user_themes.len()))
                                        .small()
                                        .color(dim(self.theme.fg(), 0.6))
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("Drop .toml files into ~/.vertex-term/themes/ to add themes")
                                        .small()
                                        .color(dim(self.theme.fg(), 0.6))
                                );
                            }
                        });
                        ui.end_row();

                        ui.label("Font size");
                        ui.add(egui::Slider::new(&mut self.config.font_size, 8.0..=32.0).suffix(" px"));
                        ui.end_row();

                        ui.label("Opacity");
                        ui.add(egui::Slider::new(&mut self.config.opacity, 0.1..=1.0));
                        ui.end_row();

                        ui.label("System borders");
                        ui.checkbox(&mut self.config.system_borders, "(restart to apply)");
                        ui.end_row();

                        // TODO: blur UI hidden until KDE Plasma 6 support is finished
                        // ui.label("Enable blur");
                        // ui.vertical(|ui| {
                        //     ui.checkbox(&mut self.config.enable_blur, "Blur window background");
                        //     ui.label(
                        //         egui::RichText::new("KDE Plasma only.")
                        //             .small()
                        //             .color(dim(self.theme.fg(), 0.6)),
                        //     );
                        // });
                        // ui.end_row();

                        ui.label("Cursor blink");
                        ui.checkbox(&mut self.config.cursor_blink, "");
                        ui.end_row();

                        ui.label("Scrollback lines");
                        ui.add(egui::Slider::new(&mut self.config.scrollback_lines, 100..=50000));
                        ui.end_row();

                        ui.label("Shell");
                        ui.vertical(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.shell_input)
                                    .desired_width(220.0)
                                    .hint_text("/usr/bin/bash"),
                            );
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                for shell in &["/usr/bin/bash", "/usr/bin/zsh", "/usr/bin/fish", "/bin/sh"] {
                                    if ui.small_button(*shell).clicked() {
                                        self.shell_input = shell.to_string();
                                    }
                                }
                            });
                        });
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("  Save  ").clicked() {
                        self.config.shell = self.shell_input.trim().to_string();
                        self.config.save();
                        blur::apply(self.config.enable_blur);
                        self.show_settings = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_settings = false;
                    }
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label("Session");
                ui.add_space(4.0);
                if ui.button("⟳  Restart Terminal Session").clicked() {
                    self.show_settings = false;
                    restart = true;
                }
                ui.label(
                    egui::RichText::new("Kills the current shell and starts a fresh one.\nClears all output and resets environment.")
                        .small()
                        .color(dim(self.theme.fg(), 0.6))
                );
            });
        restart
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(Key::PageUp) && i.modifiers.shift {
                self.terminal.scroll_offset =
                    (self.terminal.scroll_offset + self.terminal.rows / 2)
                        .min(self.terminal.scrollback.len());
            }
            if i.key_pressed(Key::PageDown) && i.modifiers.shift {
                self.terminal.scroll_offset =
                    self.terminal.scroll_offset.saturating_sub(self.terminal.rows / 2);
            }

            for event in &i.events {
                match event {
                    // egui intercepts Ctrl+C as Event::Copy before our Key handler sees it.
                    // For a terminal, Ctrl+C must always send SIGINT (0x03) to the PTY.
                    egui::Event::Copy => {
                        self.pty.write_bytes(&[0x03]);
                        self.terminal.scroll_offset = 0;
                    }
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        let bytes = key_to_bytes(*key, *modifiers);
                        if !bytes.is_empty() {
                            self.pty.write_bytes(&bytes);
                            self.terminal.scroll_offset = 0;
                        }
                    }
                    egui::Event::Text(text) => {
                        self.pty.write_bytes(text.as_bytes());
                        self.terminal.scroll_offset = 0;
                    }
                    egui::Event::Paste(text) => {
                        self.pty.write_bytes(b"\x1b[200~");
                        self.pty.write_bytes(text.as_bytes());
                        self.pty.write_bytes(b"\x1b[201~");
                    }
                    _ => {}
                }
            }
        });
    }

    fn open_settings(&mut self) {
        self.shell_input = self.config.shell.clone();
        // Re-scan the themes folder so newly dropped files show up immediately.
        self.user_themes = theme::load_user_themes();
    }

    fn restart_session(&mut self) {
        let cols = self.terminal.cols.max(4) as u16;
        let rows = self.terminal.rows.max(2) as u16;
        self.terminal = Terminal::new(cols as usize, rows as usize, self.config.scrollback_lines);
        self.parser = Parser::new();
        self.cursor_visible = true;
        self.last_blink = std::time::Instant::now();
        if let Ok(pty) = Pty::spawn(&self.config.shell, cols, rows) {
            self.pty = pty;
        }
    }
}

impl eframe::App for VertexTerm {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Always fully transparent — the CentralPanel fill provides the tinted background.
        // Keeping this transparent means the panel fill (with opacity) is the only
        // source of background colour, so opacity actually works.
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_pty_output();
        self.handle_keyboard(ctx);

        // Apply KDE blur once the window is mapped (after the first rendered frame).
        if !self.blur_applied {
            self.blur_applied = true;
            if self.config.enable_blur {
                blur::apply(true);
            }
        }

        // Cursor blink
        if self.config.cursor_blink {
            let elapsed = self.last_blink.elapsed();
            if elapsed >= std::time::Duration::from_millis(500) {
                self.cursor_visible = !self.cursor_visible;
                self.last_blink = std::time::Instant::now();
            }
        } else {
            self.cursor_visible = true;
        }

        // Consume Tab from egui's event queue so it can't cycle widget focus.
        // handle_keyboard() already forwarded it to the PTY as autocomplete.
        ctx.input_mut(|i| {
            i.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Tab, .. }));
        });

        self.apply_visuals(ctx);

        let bg = self.theme.bg_with_alpha(self.config.opacity);
        let fg = self.theme.fg();
        let no_frame = egui::Frame::none().fill(bg).inner_margin(Margin::ZERO);
        let toolbar_frame = egui::Frame::none()
            .fill(dim(bg, 0.85))
            .inner_margin(Margin::symmetric(8.0, 4.0));

        if !self.config.system_borders {
            // CSD mode: draw our own title bar and make it draggable.
            egui::TopBottomPanel::top("titlebar")
                .frame(toolbar_frame)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        // Drag region — takes all space not used by buttons.
                        let drag_resp = ui.interact(
                            ui.available_rect_before_wrap(),
                            egui::Id::new("titlebar_drag"),
                            egui::Sense::click_and_drag(),
                        );
                        ui.label(egui::RichText::new("  Vertex Term").color(fg).strong());
                        if drag_resp.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(" ✕ ").clicked() { std::process::exit(0); }
                            if ui.button(" ⚙ ").clicked() {
                                self.show_settings = !self.show_settings;
                                if self.show_settings { self.open_settings(); }
                            }
                        });
                    });
                });
        } else {
            egui::TopBottomPanel::top("toolbar")
                .frame(toolbar_frame)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("⚙ Settings").clicked() {
                            self.show_settings = !self.show_settings;
                            if self.show_settings { self.open_settings(); }
                        }
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(no_frame)
            .show(ctx, |ui| {
                self.render_terminal(ui);
            });

        if self.show_settings {
            if self.settings_panel(ctx) {
                self.restart_session();
            }
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Returns (fg, bg, italic, underline) for a single cell — used for style-run batching.
///
/// Default cell backgrounds are TRANSPARENT so the CentralPanel fill (which carries the
/// opacity setting) shows through uniformly.  Explicit ANSI/RGB backgrounds have the
/// opacity blended in so they stay visually consistent with the rest of the terminal.
fn cell_style(
    cell: &crate::terminal::Cell,
    col_idx: usize,
    is_active_row: bool,
    cursor_col: usize,
    cursor_visible: bool,
    theme: &crate::theme::Theme,
    opacity: f32,
) -> (Color32, Color32, bool, bool) {
    use crate::terminal::CellColor;

    let is_cursor = is_active_row && col_idx == cursor_col && cursor_visible;

    let resolve_fg = |c: &CellColor| c.resolve(theme.fg(), |i| theme.ansi_color(i));
    let resolve_bg = |c: &CellColor| -> Color32 {
        match c {
            CellColor::Default => Color32::TRANSPARENT,
            other => with_opacity(other.resolve(theme.bg(), |i| theme.ansi_color(i)), opacity),
        }
    };

    let (mut fg, mut bg) = if cell.inverse {
        (resolve_bg(&cell.fg), resolve_fg(&cell.bg))
    } else {
        (resolve_fg(&cell.fg), resolve_bg(&cell.bg))
    };

    if is_cursor {
        std::mem::swap(&mut fg, &mut bg);
        bg = with_opacity(theme.cursor_color(), opacity.max(0.6));
    }
    (fg, bg, cell.italic, cell.underline)
}

fn with_opacity(c: Color32, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (opacity.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Blend a color toward black by `factor` (1.0 = unchanged, 0.0 = black).
fn dim(c: Color32, factor: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c.r() as f32 * factor) as u8,
        (c.g() as f32 * factor) as u8,
        (c.b() as f32 * factor) as u8,
        c.a(),
    )
}

fn key_to_bytes(key: Key, mods: Modifiers) -> Vec<u8> {
    if mods.ctrl && !mods.shift && !mods.alt {
        let byte = match key {
            Key::A => Some(0x01), Key::B => Some(0x02), Key::C => Some(0x03),
            Key::D => Some(0x04), Key::E => Some(0x05), Key::F => Some(0x06),
            Key::G => Some(0x07), Key::H => Some(0x08), Key::I => Some(0x09),
            Key::J => Some(0x0A), Key::K => Some(0x0B), Key::L => Some(0x0C),
            Key::M => Some(0x0D), Key::N => Some(0x0E), Key::O => Some(0x0F),
            Key::P => Some(0x10), Key::Q => Some(0x11), Key::R => Some(0x12),
            Key::S => Some(0x13), Key::T => Some(0x14), Key::U => Some(0x15),
            Key::V => Some(0x16), Key::W => Some(0x17), Key::X => Some(0x18),
            Key::Y => Some(0x19), Key::Z => Some(0x1A),
            Key::OpenBracket => Some(0x1B),
            Key::Backslash   => Some(0x1C),
            Key::CloseBracket => Some(0x1D),
            _ => None,
        };
        if let Some(b) = byte { return vec![b]; }
    }

    match key {
        Key::Enter     => vec![b'\r'],
        Key::Backspace => vec![0x7F],
        Key::Escape    => vec![0x1B],
        Key::Tab       => vec![b'\t'],
        Key::ArrowUp    => esc(b'A'),
        Key::ArrowDown  => esc(b'B'),
        Key::ArrowRight => esc(b'C'),
        Key::ArrowLeft  => esc(b'D'),
        Key::Home       => vec![0x1B, b'[', b'H'],
        Key::End        => vec![0x1B, b'[', b'F'],
        Key::PageUp     => vec![0x1B, b'[', b'5', b'~'],
        Key::PageDown   => vec![0x1B, b'[', b'6', b'~'],
        Key::Delete     => vec![0x1B, b'[', b'3', b'~'],
        Key::Insert     => vec![0x1B, b'[', b'2', b'~'],
        Key::F1  => vec![0x1B, b'O', b'P'],
        Key::F2  => vec![0x1B, b'O', b'Q'],
        Key::F3  => vec![0x1B, b'O', b'R'],
        Key::F4  => vec![0x1B, b'O', b'S'],
        Key::F5  => vec![0x1B, b'[', b'1', b'5', b'~'],
        Key::F6  => vec![0x1B, b'[', b'1', b'7', b'~'],
        Key::F7  => vec![0x1B, b'[', b'1', b'8', b'~'],
        Key::F8  => vec![0x1B, b'[', b'1', b'9', b'~'],
        Key::F9  => vec![0x1B, b'[', b'2', b'0', b'~'],
        Key::F10 => vec![0x1B, b'[', b'2', b'1', b'~'],
        Key::F11 => vec![0x1B, b'[', b'2', b'3', b'~'],
        Key::F12 => vec![0x1B, b'[', b'2', b'4', b'~'],
        _ => vec![],
    }
}

fn esc(suffix: u8) -> Vec<u8> { vec![0x1B, b'[', suffix] }
