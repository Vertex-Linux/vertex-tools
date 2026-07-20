use egui::Color32;
use vte::{Params, Perform};

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub ch: char,
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum CellColor {
    #[default]
    Default,
    Ansi(u8),
    Rgb(u8, u8, u8),
}

impl CellColor {
    pub fn resolve(&self, default: Color32, theme_ansi: impl Fn(usize) -> Color32) -> Color32 {
        match self {
            CellColor::Default => default,
            CellColor::Ansi(i) => theme_ansi(*i as usize),
            CellColor::Rgb(r, g, b) => Color32::from_rgb(*r, *g, *b),
        }
    }
}

#[derive(Debug)]
pub struct Terminal {
    pub cols: usize,
    pub rows: usize,
    pub grid: Vec<Vec<Cell>>,
    pub scrollback: Vec<Vec<Cell>>,
    pub scrollback_limit: usize,
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub scroll_offset: usize, // lines scrolled up from bottom

    // Current SGR state
    cur_fg: CellColor,
    cur_bg: CellColor,
    cur_bold: bool,
    cur_italic: bool,
    cur_underline: bool,
    cur_inverse: bool,

    // Saved cursor
    saved_col: usize,
    saved_row: usize,

    // DEC private mode: set when shell sends \x1b[?2004h
    pub bracketed_paste: bool,

    // DEC private mode 25 (DECTCEM): whether the app wants the cursor drawn at all.
    pub cursor_visible: bool,

    // Alternate screen buffer (DEC private modes 47/1049), used by full-screen
    // TUI apps (ratatui/crossterm, vim, less, htop, ...) so they can repaint
    // freely without disturbing scrollback, then hand the original screen back
    // untouched on exit.
    in_alt_screen: bool,
    saved_grid: Option<Vec<Vec<Cell>>>,
    saved_cursor: Option<(usize, usize)>,

    // Pending byte buffer (for multi-byte UTF-8 from PTY reads)
    pub pending: Vec<u8>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback_limit: usize) -> Self {
        Self {
            cols,
            rows,
            grid: vec![vec![Cell::default(); cols]; rows],
            scrollback: Vec::new(),
            scrollback_limit,
            cursor_col: 0,
            cursor_row: 0,
            scroll_offset: 0,
            cur_fg: CellColor::Default,
            cur_bg: CellColor::Default,
            cur_bold: false,
            cur_italic: false,
            cur_underline: false,
            cur_inverse: false,
            saved_col: 0,
            saved_row: 0,
            bracketed_paste: false,
            cursor_visible: true,
            in_alt_screen: false,
            saved_grid: None,
            saved_cursor: None,
            pending: Vec::new(),
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        for row in &mut self.grid {
            row.resize(cols, Cell::default());
        }
        self.grid.resize(rows, vec![Cell::default(); cols]);
        // Keep the off-screen buffer in sync too, so it's still the right shape
        // if the app leaves the alt screen after a resize.
        if let Some(saved) = &mut self.saved_grid {
            for row in saved.iter_mut() {
                row.resize(cols, Cell::default());
            }
            saved.resize(rows, vec![Cell::default(); cols]);
        }
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
    }

    /// DEC private mode 47/1049 set: switch to a blank alternate screen,
    /// stashing the primary screen's contents and cursor to restore later.
    fn enter_alt_screen(&mut self) {
        if self.in_alt_screen { return; }
        self.in_alt_screen = true;
        self.saved_grid = Some(std::mem::replace(
            &mut self.grid,
            vec![vec![Cell::default(); self.cols]; self.rows],
        ));
        self.saved_cursor = Some((self.cursor_row, self.cursor_col));
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.scroll_offset = 0;
    }

    /// DEC private mode 47/1049 reset: hand the primary screen back exactly
    /// as the full-screen app found it.
    fn leave_alt_screen(&mut self) {
        if !self.in_alt_screen { return; }
        self.in_alt_screen = false;
        if let Some(grid) = self.saved_grid.take() {
            self.grid = grid;
        }
        if let Some((row, col)) = self.saved_cursor.take() {
            self.cursor_row = row.min(self.rows.saturating_sub(1));
            self.cursor_col = col.min(self.cols.saturating_sub(1));
        }
        self.scroll_offset = 0;
    }

    fn current_cell(&self) -> Cell {
        Cell {
            ch: ' ',
            fg: self.cur_fg.clone(),
            bg: self.cur_bg.clone(),
            bold: self.cur_bold,
            italic: self.cur_italic,
            underline: self.cur_underline,
            inverse: self.cur_inverse,
        }
    }

    fn scroll_up(&mut self) {
        let row = std::mem::replace(&mut self.grid[0], vec![Cell::default(); self.cols]);
        self.grid.remove(0);
        self.grid.push(vec![Cell::default(); self.cols]);

        // Full-screen apps (TUIs) repaint the whole screen themselves and don't
        // expect their internal scrolling to leak into the primary scrollback.
        if self.in_alt_screen { return; }

        if self.scrollback.len() >= self.scrollback_limit {
            self.scrollback.remove(0);
        }
        self.scrollback.push(row);
    }

    fn newline(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_row += 1;
        }
    }

    fn apply_sgr(&mut self, params: &Params) {
        let mut iter = params.iter();
        loop {
            let p = match iter.next() {
                Some(s) => s,
                None => break,
            };
            let code = p.first().copied().unwrap_or(0);
            match code {
                0 => {
                    self.cur_fg = CellColor::Default;
                    self.cur_bg = CellColor::Default;
                    self.cur_bold = false;
                    self.cur_italic = false;
                    self.cur_underline = false;
                    self.cur_inverse = false;
                }
                1 => self.cur_bold = true,
                3 => self.cur_italic = true,
                4 => self.cur_underline = true,
                7 => self.cur_inverse = true,
                22 => self.cur_bold = false,
                23 => self.cur_italic = false,
                24 => self.cur_underline = false,
                27 => self.cur_inverse = false,
                30..=37 => self.cur_fg = CellColor::Ansi((code - 30) as u8),
                38 => {
                    // next subparams: 2;r;g;b or 5;n
                    if let Some(sub) = iter.next() {
                        match sub.first().copied() {
                            Some(2) => {
                                // truecolor — pull three more
                                let r = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                let g = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                let b = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                self.cur_fg = CellColor::Rgb(r as u8, g as u8, b as u8);
                            }
                            Some(5) => {
                                let n = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                self.cur_fg = CellColor::Ansi(n as u8);
                            }
                            _ => {}
                        }
                    }
                }
                39 => self.cur_fg = CellColor::Default,
                40..=47 => self.cur_bg = CellColor::Ansi((code - 40) as u8),
                48 => {
                    if let Some(sub) = iter.next() {
                        match sub.first().copied() {
                            Some(2) => {
                                let r = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                let g = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                let b = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                self.cur_bg = CellColor::Rgb(r as u8, g as u8, b as u8);
                            }
                            Some(5) => {
                                let n = iter.next().and_then(|s| s.first().copied()).unwrap_or(0);
                                self.cur_bg = CellColor::Ansi(n as u8);
                            }
                            _ => {}
                        }
                    }
                }
                49 => self.cur_bg = CellColor::Default,
                90..=97 => self.cur_fg = CellColor::Ansi((code - 90 + 8) as u8),
                100..=107 => self.cur_bg = CellColor::Ansi((code - 100 + 8) as u8),
                _ => {}
            }
        }
    }
}

impl Perform for Terminal {
    fn print(&mut self, c: char) {
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.newline();
        }
        let mut cell = self.current_cell();
        cell.ch = c;
        self.grid[self.cursor_row][self.cursor_col] = cell;
        self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.newline(),
            b'\r' => self.cursor_col = 0,
            b'\x08' => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            b'\t' => {
                let next = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = next.min(self.cols - 1);
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let p: Vec<u16> = params.iter()
            .filter_map(|s| s.first().copied())
            .collect();
        let p0 = *p.first().unwrap_or(&0) as usize;
        let p1 = *p.get(1).unwrap_or(&0) as usize;

        // DEC private modes: \x1b[?<n>h / \x1b[?<n>l — a single escape can set/reset
        // several modes at once (e.g. \x1b[?1000;1002;1003;1006h for mouse tracking),
        // so walk every code rather than just the first.
        if intermediates.first() == Some(&b'?') {
            let enable = action == 'h';
            for &code in &p {
                match code {
                    25 => self.cursor_visible = enable,
                    47 | 1047 | 1049 => {
                        if enable { self.enter_alt_screen(); } else { self.leave_alt_screen(); }
                    }
                    2004 => self.bracketed_paste = enable,
                    _ => {}
                }
            }
            return;
        }

        match action {
            'A' => self.cursor_row = self.cursor_row.saturating_sub(p0.max(1)),
            'B' => self.cursor_row = (self.cursor_row + p0.max(1)).min(self.rows - 1),
            'C' => self.cursor_col = (self.cursor_col + p0.max(1)).min(self.cols - 1),
            'D' => self.cursor_col = self.cursor_col.saturating_sub(p0.max(1)),
            'E' => { self.cursor_row = (self.cursor_row + p0.max(1)).min(self.rows - 1); self.cursor_col = 0; }
            'F' => { self.cursor_row = self.cursor_row.saturating_sub(p0.max(1)); self.cursor_col = 0; }
            'G' => self.cursor_col = (p0.max(1) - 1).min(self.cols - 1),
            'H' | 'f' => {
                self.cursor_row = (p0.max(1) - 1).min(self.rows - 1);
                self.cursor_col = (p1.max(1) - 1).min(self.cols - 1);
            }
            'J' => match p0 {
                0 => {
                    for c in self.cursor_col..self.cols { self.grid[self.cursor_row][c] = Cell::default(); }
                    for r in (self.cursor_row + 1)..self.rows { self.grid[r] = vec![Cell::default(); self.cols]; }
                }
                1 => {
                    for c in 0..=self.cursor_col { self.grid[self.cursor_row][c] = Cell::default(); }
                    for r in 0..self.cursor_row { self.grid[r] = vec![Cell::default(); self.cols]; }
                }
                2 => {
                    for r in 0..self.rows { self.grid[r] = vec![Cell::default(); self.cols]; }
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                }
                3 => {
                    // Erase saved lines (scrollback) — sent by modern `clear`
                    self.scrollback.clear();
                }
                _ => {}
            },
            'K' => match p0 {
                0 => for c in self.cursor_col..self.cols { self.grid[self.cursor_row][c] = Cell::default(); },
                1 => for c in 0..=self.cursor_col { self.grid[self.cursor_row][c] = Cell::default(); },
                2 => self.grid[self.cursor_row] = vec![Cell::default(); self.cols],
                _ => {}
            },
            'L' => {
                for _ in 0..p0.max(1) {
                    self.grid.insert(self.cursor_row, vec![Cell::default(); self.cols]);
                    if self.grid.len() > self.rows { self.grid.pop(); }
                }
            }
            'M' => {
                for _ in 0..p0.max(1) {
                    if self.cursor_row < self.grid.len() { self.grid.remove(self.cursor_row); }
                    self.grid.push(vec![Cell::default(); self.cols]);
                }
            }
            'P' => {
                let row = &mut self.grid[self.cursor_row];
                let n = p0.max(1).min(self.cols - self.cursor_col);
                row.drain(self.cursor_col..self.cursor_col + n);
                row.resize(self.cols, Cell::default());
            }
            'S' => for _ in 0..p0.max(1) { self.scroll_up(); },
            'T' => for _ in 0..p0.max(1) {
                self.grid.insert(0, vec![Cell::default(); self.cols]);
                if self.grid.len() > self.rows { self.scrollback.push(self.grid.pop().unwrap()); }
            },
            'X' => {
                let n = p0.max(1).min(self.cols - self.cursor_col);
                for c in self.cursor_col..self.cursor_col + n { self.grid[self.cursor_row][c] = Cell::default(); }
            }
            'd' => self.cursor_row = (p0.max(1) - 1).min(self.rows - 1),
            'm' => self.apply_sgr(params),
            's' => { self.saved_col = self.cursor_col; self.saved_row = self.cursor_row; }
            'u' => { self.cursor_col = self.saved_col; self.cursor_row = self.saved_row; }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => { self.saved_col = self.cursor_col; self.saved_row = self.cursor_row; }
            b'8' => { self.cursor_col = self.saved_col; self.cursor_row = self.saved_row; }
            b'M' => { // reverse index
                if self.cursor_row == 0 {
                    self.grid.insert(0, vec![Cell::default(); self.cols]);
                    if self.grid.len() > self.rows { self.grid.pop(); }
                } else {
                    self.cursor_row -= 1;
                }
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
}
