use egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub cursor: [u8; 3],
    pub selection: [u8; 3],
    /// ANSI colors 0-15
    pub ansi: [[u8; 3]; 16],
}

impl Theme {
    pub fn bg(&self) -> Color32 { rgb(self.background) }
    pub fn fg(&self) -> Color32 { rgb(self.foreground) }
    pub fn cursor_color(&self) -> Color32 { rgb(self.cursor) }
    pub fn selection_color(&self) -> Color32 { rgb(self.selection) }
    pub fn ansi_color(&self, idx: usize) -> Color32 { rgb(self.ansi[idx.min(15)]) }
    pub fn bg_with_alpha(&self, opacity: f32) -> Color32 {
        let [r, g, b] = self.background;
        Color32::from_rgba_unmultiplied(r, g, b, (opacity.clamp(0.0, 1.0) * 255.0) as u8)
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color32 { Color32::from_rgb(r, g, b) }

// ── User theme file format ────────────────────────────────────────────────────

/// On-disk representation of a theme.  Colors are `"#rrggbb"` hex strings so
/// theme files are easy to write by hand or copy from any terminal colour scheme
/// website (base16, Gogh, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFile {
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection: String,
    /// Exactly 16 hex colors: indices 0-7 normal, 8-15 bright.
    pub ansi: Vec<String>,
}

impl ThemeFile {
    pub fn to_theme(&self) -> Option<Theme> {
        if self.ansi.len() < 16 { return None; }
        let mut ansi = [[0u8; 3]; 16];
        for (i, s) in self.ansi.iter().enumerate().take(16) {
            ansi[i] = parse_hex(s)?;
        }
        Some(Theme {
            name: self.name.clone(),
            background: parse_hex(&self.background)?,
            foreground: parse_hex(&self.foreground)?,
            cursor:     parse_hex(&self.cursor)?,
            selection:  parse_hex(&self.selection)?,
            ansi,
        })
    }
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 { return None; }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some([r, g, b])
}

// ── Directory management ──────────────────────────────────────────────────────

fn user_theme_dir() -> Option<std::path::PathBuf> {
    dirs_next::home_dir().map(|h| h.join(".vertex-term").join("themes"))
}

/// Create `~/.vertex-term/themes/` on first launch and seed it with a template
/// and a couple of ready-to-use example themes.
pub fn init_user_theme_dir() {
    let Some(dir) = user_theme_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);

    // Only write seeds if the directory is brand-new (no .toml files yet).
    let is_empty = std::fs::read_dir(&dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true);
    if !is_empty { return; }

    let seeds: &[(&str, &str)] = &[
        ("template.toml",    TEMPLATE_TOML),
        ("nord.toml",        NORD_TOML),
        ("tokyo-night.toml", TOKYO_NIGHT_TOML),
        ("one-dark.toml",    ONE_DARK_TOML),
    ];
    for (filename, content) in seeds {
        let _ = std::fs::write(dir.join(filename), content);
    }
}

/// Scan `~/.vertex-term/themes/` and return all valid themes found there.
pub fn load_user_themes() -> Vec<Theme> {
    let Some(dir) = user_theme_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("toml"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| toml::from_str::<ThemeFile>(&s).ok())
        .filter_map(|tf| tf.to_theme())
        .collect()
}

pub fn all_themes(user: &[Theme]) -> Vec<Theme> {
    let mut all = builtin_themes();
    for t in user {
        // Don't override a built-in with the same name.
        if !all.iter().any(|b| b.name == t.name) {
            all.push(t.clone());
        }
    }
    all
}

pub fn builtin_themes() -> Vec<Theme> {
    vec![dark(), light(), gruvbox(), dracula(), solarized_dark()]
}

pub fn by_name(name: &str) -> Theme {
    let user = load_user_themes();
    all_themes(&user)
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(dark)
}

fn dark() -> Theme {
    Theme {
        name: "dark".into(),
        background: [0x1e, 0x1e, 0x2e],
        foreground: [0xcd, 0xd6, 0xf4],
        cursor:     [0xf5, 0xc2, 0xe7],
        selection:  [0x58, 0x5b, 0x70],
        ansi: [
            [0x45, 0x47, 0x5a], // black
            [0xf3, 0x8b, 0xa8], // red
            [0xa6, 0xe3, 0xa1], // green
            [0xf9, 0xe2, 0xaf], // yellow
            [0x89, 0xb4, 0xfa], // blue
            [0xcb, 0xa6, 0xf7], // magenta
            [0x94, 0xe2, 0xd5], // cyan
            [0xba, 0xc2, 0xde], // white
            [0x58, 0x5b, 0x70], // bright black
            [0xf3, 0x8b, 0xa8], // bright red
            [0xa6, 0xe3, 0xa1], // bright green
            [0xf9, 0xe2, 0xaf], // bright yellow
            [0x89, 0xb4, 0xfa], // bright blue
            [0xcb, 0xa6, 0xf7], // bright magenta
            [0x94, 0xe2, 0xd5], // bright cyan
            [0xa6, 0xad, 0xc8], // bright white
        ],
    }
}

fn light() -> Theme {
    Theme {
        name: "light".into(),
        background: [0xef, 0xf1, 0xf5],
        foreground: [0x4c, 0x4f, 0x69],
        cursor:     [0xdc, 0x8a, 0x78],
        selection:  [0xac, 0xc0, 0xe4],
        ansi: [
            [0x5c, 0x5f, 0x77],
            [0xd2, 0x0f, 0x39],
            [0x40, 0xa0, 0x2b],
            [0xdf, 0x8e, 0x1d],
            [0x1e, 0x66, 0xf5],
            [0xea, 0x76, 0xcb],
            [0x17, 0x94, 0x99],
            [0xac, 0xb0, 0xbe],
            [0x6c, 0x6f, 0x85],
            [0xd2, 0x0f, 0x39],
            [0x40, 0xa0, 0x2b],
            [0xdf, 0x8e, 0x1d],
            [0x1e, 0x66, 0xf5],
            [0xea, 0x76, 0xcb],
            [0x17, 0x94, 0x99],
            [0xbc, 0xbe, 0xcc],
        ],
    }
}

fn gruvbox() -> Theme {
    Theme {
        name: "gruvbox".into(),
        background: [0x28, 0x28, 0x28],
        foreground: [0xeb, 0xdb, 0xb2],
        cursor:     [0xfb, 0xf1, 0xc7],
        selection:  [0x3c, 0x38, 0x36],
        ansi: [
            [0x28, 0x28, 0x28],
            [0xcc, 0x24, 0x1d],
            [0x98, 0x97, 0x1a],
            [0xd7, 0x99, 0x21],
            [0x45, 0x85, 0x88],
            [0xb1, 0x62, 0x86],
            [0x68, 0x9d, 0x6a],
            [0xa8, 0x99, 0x84],
            [0x92, 0x83, 0x74],
            [0xfb, 0x49, 0x34],
            [0xb8, 0xbb, 0x26],
            [0xfa, 0xbd, 0x2f],
            [0x83, 0xa5, 0x98],
            [0xd3, 0x86, 0x9b],
            [0x8e, 0xc0, 0x7c],
            [0xeb, 0xdb, 0xb2],
        ],
    }
}

fn dracula() -> Theme {
    Theme {
        name: "dracula".into(),
        background: [0x28, 0x2a, 0x36],
        foreground: [0xf8, 0xf8, 0xf2],
        cursor:     [0xf8, 0xf8, 0xf2],
        selection:  [0x44, 0x47, 0x5a],
        ansi: [
            [0x21, 0x22, 0x2c],
            [0xff, 0x55, 0x55],
            [0x50, 0xfa, 0x7b],
            [0xf1, 0xfa, 0x8c],
            [0xbd, 0x93, 0xf9],
            [0xff, 0x79, 0xc6],
            [0x8b, 0xe9, 0xfd],
            [0xf8, 0xf8, 0xf2],
            [0x62, 0x72, 0xa4],
            [0xff, 0x66, 0x6d],
            [0x69, 0xff, 0x94],
            [0xff, 0xff, 0xa5],
            [0xd6, 0xac, 0xff],
            [0xff, 0x92, 0xdf],
            [0xa4, 0xff, 0xff],
            [0xff, 0xff, 0xff],
        ],
    }
}

fn solarized_dark() -> Theme {
    Theme {
        name: "solarized-dark".into(),
        background: [0x00, 0x2b, 0x36],
        foreground: [0x83, 0x94, 0x96],
        cursor:     [0x93, 0xa1, 0xa1],
        selection:  [0x07, 0x36, 0x42],
        ansi: [
            [0x07, 0x36, 0x42],
            [0xdc, 0x32, 0x2f],
            [0x85, 0x99, 0x00],
            [0xb5, 0x89, 0x00],
            [0x26, 0x8b, 0xd2],
            [0xd3, 0x36, 0x82],
            [0x2a, 0xa1, 0x98],
            [0xee, 0xe8, 0xd5],
            [0x00, 0x2b, 0x36],
            [0xcb, 0x4b, 0x16],
            [0x58, 0x6e, 0x75],
            [0x65, 0x7b, 0x83],
            [0x83, 0x94, 0x96],
            [0x6c, 0x71, 0xc4],
            [0x93, 0xa1, 0xa1],
            [0xfd, 0xf6, 0xe3],
        ],
    }
}

// ── Seed theme files written on first launch ──────────────────────────────────

const TEMPLATE_TOML: &str = r##"# Vertex Term custom theme
# Drop any .toml file that follows this format into ~/.vertex-term/themes/
# and it will appear in Settings > Theme.
#
# All colors are hex strings: "#rrggbb" — the leading # is optional.

name = "my-theme"

background = "#1e1e2e"   # terminal canvas
foreground = "#cdd6f4"   # default text
cursor     = "#f5c2e7"   # cursor block
selection  = "#585b70"   # selected-text highlight

# 16 ANSI colors in order:
#   0  black      1  red        2  green      3  yellow
#   4  blue       5  magenta    6  cyan       7  white
#   8  brblack    9  brred     10  brgreen   11  bryellow
#  12  brblue    13  brmagenta 14  brcyan    15  brwhite
ansi = [
    "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
    "#89b4fa", "#cba6f7", "#94e2d5", "#bac2de",
    "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af",
    "#89b4fa", "#cba6f7", "#94e2d5", "#a6adc8",
]
"##;

const NORD_TOML: &str = r##"name = "nord"
background = "#2e3440"
foreground = "#d8dee9"
cursor     = "#d8dee9"
selection  = "#434c5e"
ansi = [
    "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b",
    "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
    "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b",
    "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
]
"##;

const TOKYO_NIGHT_TOML: &str = r##"name = "tokyo-night"
background = "#1a1b26"
foreground = "#c0caf5"
cursor     = "#c0caf5"
selection  = "#283457"
ansi = [
    "#15161e", "#f7768e", "#9ece6a", "#e0af68",
    "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
    "#414868", "#f7768e", "#9ece6a", "#e0af68",
    "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
]
"##;

const ONE_DARK_TOML: &str = r##"name = "one-dark"
background = "#282c34"
foreground = "#abb2bf"
cursor     = "#528bff"
selection  = "#3e4451"
ansi = [
    "#3f4451", "#e06c75", "#98c379", "#e5c07b",
    "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
    "#4f5666", "#e06c75", "#98c379", "#e5c07b",
    "#61afef", "#c678dd", "#56b6c2", "#ffffff",
]
"##;
