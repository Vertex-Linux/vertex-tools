use crate::package::Source;
use crate::tui::app::{App, Mode};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame,
};

const LOGO: &[&str] = &[
    "██╗   ██╗██████╗ ██╗  ██╗ ██████╗",
    "██║   ██║██╔══██╗██║ ██╔╝██╔════╝",
    "██║   ██║██████╔╝█████╔╝ ██║  ███╗",
    "╚██╗ ██╔╝██╔═══╝ ██╔═██╗ ██║   ██║",
    " ╚████╔╝ ██║     ██║  ██╗╚██████╔╝",
    "  ╚═══╝  ╚═╝     ╚═╝  ╚═╝ ╚═════╝",
];

fn gradient_color(col: usize, total: usize) -> Color {
    // Purple (168,85,247) → Cyan (6,182,212)
    let t = if total == 0 { 0.0 } else { col as f64 / total as f64 };
    let r = (168.0 + (6.0 - 168.0) * t) as u8;
    let g = (85.0 + (182.0 - 85.0) * t) as u8;
    let b = (247.0 + (212.0 - 247.0) * t) as u8;
    Color::Rgb(r, g, b)
}

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let logo_height = LOGO.len() as u16 + 2; // +2 for subtitle + gap

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(logo_height), // header
            Constraint::Length(3),           // search box
            Constraint::Min(3),              // results
            Constraint::Length(1),           // status bar
        ])
        .split(area);

    draw_header(f, chunks[0], app);
    draw_search(f, chunks[1], app);
    draw_results(f, chunks[2], app);
    draw_status(f, chunks[3], app);

    if let Mode::SourcePick(ref sources) = app.mode {
        let sources = sources.clone();
        draw_source_picker(f, area, &sources);
    }

    if app.mode == Mode::Help {
        draw_help(f, area);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let filter_label = match &app.filter {
        None => " ALL ".to_string(),
        Some(s) => format!(" {} ", s.label()),
    };

    let mut lines: Vec<Line> = LOGO
        .iter()
        .map(|row| {
            let total = row.chars().count();
            let spans: Vec<Span> = row
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    Span::styled(
                        c.to_string(),
                        Style::default().fg(gradient_color(i, total)),
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    lines.push(Line::from(vec![
        Span::styled("vertex package manager", Style::default().fg(Color::Rgb(148, 163, 184))),
        Span::raw("  "),
        Span::styled(
            filter_label,
            Style::default()
                .fg(Color::Rgb(15, 23, 42))
                .bg(Color::Rgb(168, 85, 247))
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let para = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(para, area);
}

fn draw_search(f: &mut Frame, area: Rect, app: &App) {
    let spinner = if app.is_loading {
        format!(" {} ", app.spinner_frame())
    } else {
        " > ".to_string()
    };

    let spinner_color = if app.is_loading {
        Color::Rgb(234, 179, 8)
    } else {
        Color::Rgb(168, 85, 247)
    };

    let input_line = Line::from(vec![
        Span::styled(spinner, Style::default().fg(spinner_color).add_modifier(Modifier::BOLD)),
        Span::styled(
            app.search_input.as_str(),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled("█", Style::default().fg(Color::Rgb(168, 85, 247))),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(99, 102, 241)))
        .title(Span::styled(
            " Search ",
            Style::default()
                .fg(Color::Rgb(168, 85, 247))
                .add_modifier(Modifier::BOLD),
        ));

    let para = Paragraph::new(input_line).block(block);
    f.render_widget(para, area);
}

fn draw_results(f: &mut Frame, area: Rect, app: &mut App) {
    let visible = app.visible();
    let count = visible.len();

    let title = if count == 0 {
        String::from(" Packages ")
    } else {
        format!(" Packages ({}) ", count)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(71, 85, 105)))
        .title(Span::styled(
            title,
            Style::default()
                .fg(Color::Rgb(148, 163, 184))
                .add_modifier(Modifier::BOLD),
        ));

    if visible.is_empty() {
        let msg = if app.search_input.is_empty() {
            "  Start typing to search across Pacman · AUR · Flatpak"
        } else if app.is_loading {
            "  Searching…"
        } else {
            "  No packages found."
        };
        let para = Paragraph::new(Span::styled(
            msg,
            Style::default().fg(Color::Rgb(100, 116, 139)),
        ))
        .block(block);
        f.render_widget(para, area);
        return;
    }

    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(i, pkg)| {
            let is_marked = app.marked.contains(&i);
            let mark = if is_marked { "●" } else { " " };

            let badge = format!("[{}]", pkg.source.label());
            let name_width = 28usize;
            let ver_width = 14usize;

            let name_truncated = truncate(&pkg.name, name_width);
            let ver_truncated = truncate(&pkg.version, ver_width);
            let desc_truncated = truncate(
                &pkg.description,
                area.width.saturating_sub(badge.len() as u16 + name_width as u16 + ver_width as u16 + 12) as usize,
            );

            let installed_dot = if pkg.installed { "✓ " } else { "  " };

            let line = Line::from(vec![
                Span::styled(
                    format!(" {} ", mark),
                    Style::default().fg(if is_marked {
                        Color::Rgb(168, 85, 247)
                    } else {
                        Color::Rgb(71, 85, 105)
                    }),
                ),
                Span::styled(
                    installed_dot,
                    Style::default().fg(Color::Rgb(34, 197, 94)),
                ),
                Span::styled(
                    badge,
                    Style::default()
                        .fg(pkg.source.color())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:<width$}", name_truncated, width = name_width),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<width$}", ver_truncated, width = ver_width),
                    Style::default().fg(Color::Rgb(100, 116, 139)),
                ),
                Span::styled(
                    desc_truncated,
                    Style::default().fg(Color::Rgb(148, 163, 184)),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(30, 27, 75))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("");

    f.render_stateful_widget(list, area, &mut state);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let line = Line::from(vec![Span::styled(
        app.status.as_str(),
        Style::default().fg(Color::Rgb(100, 116, 139)),
    )]);
    let para = Paragraph::new(line);
    f.render_widget(para, area);
}

fn draw_source_picker(f: &mut Frame, area: Rect, sources: &[Source]) {
    let popup_width = 36u16;
    let popup_height = (sources.len() as u16) + 6;
    let x = area.width.saturating_sub(popup_width) / 2;
    let y = area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(168, 85, 247)))
        .title(Span::styled(
            " Select Source ",
            Style::default()
                .fg(Color::Rgb(168, 85, 247))
                .add_modifier(Modifier::BOLD),
        ));

    let mut lines = vec![Line::from("")];
    for (i, s) in sources.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                format!("[{}] ", i + 1),
                Style::default().fg(Color::Rgb(168, 85, 247)).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                s.label(),
                Style::default().fg(s.color()).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   Press 1–9 or ESC to cancel",
        Style::default().fg(Color::Rgb(100, 116, 139)),
    )));

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, popup_area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let popup_width = 52u16;
    let popup_height = 18u16;
    let x = area.width.saturating_sub(popup_width) / 2;
    let y = area.height.saturating_sub(popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(168, 85, 247)))
        .title(Span::styled(
            " Help ",
            Style::default()
                .fg(Color::Rgb(168, 85, 247))
                .add_modifier(Modifier::BOLD),
        ));

    let key = |k: &str, desc: &str| -> Line<'static> {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:<14}", k),
                Style::default()
                    .fg(Color::Rgb(234, 179, 8))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(desc.to_string(), Style::default().fg(Color::Rgb(148, 163, 184))),
        ])
    };

    let lines = vec![
        Line::from(""),
        key("Type", "Search across all sources"),
        key("↑ / ↓", "Navigate results"),
        key("Space", "Mark/unmark package"),
        key("i / Enter", "Install selected/marked"),
        key("Tab", "Cycle source filter (All·PKG·AUR·FP)"),
        key("Backspace", "Delete last character"),
        key("ESC", "Clear search / close popup"),
        key("q", "Quit"),
        key("?", "Toggle this help"),
        Line::from(""),
        Line::from(Span::styled(
            "  ✓ = installed  ● = marked",
            Style::default().fg(Color::Rgb(100, 116, 139)),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press ? or ESC to close",
            Style::default().fg(Color::Rgb(71, 85, 105)),
        )),
    ];

    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(para, popup_area);
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let mut result: String = chars[..max.saturating_sub(1)].iter().collect();
        result.push('…');
        result
    }
}
