use crate::package::{Package, Source};
use std::collections::HashSet;
use std::time::Instant;

#[derive(Debug, PartialEq, Clone)]
pub enum Mode {
    Search,
    Browse,
    SourcePick(Vec<Source>),
    Help,
}

pub struct App {
    pub mode: Mode,
    pub search_input: String,
    pub search_results: Vec<Package>,
    pub selected: usize,
    pub marked: HashSet<usize>,
    pub filter: Option<Source>,
    pub status: String,
    pub is_loading: bool,
    pub spinner: usize,
    pub last_input: Instant,
    pub should_quit: bool,
    pub packages_to_install: Option<Vec<Package>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            mode: Mode::Search,
            search_input: String::new(),
            search_results: Vec::new(),
            selected: 0,
            marked: HashSet::new(),
            filter: None,
            status: String::from(" Type to search · Tab: filter · ? help · q quit"),
            is_loading: false,
            spinner: 0,
            last_input: Instant::now(),
            should_quit: false,
            packages_to_install: None,
        }
    }

    pub fn visible(&self) -> Vec<&Package> {
        self.search_results
            .iter()
            .filter(|p| self.filter.as_ref().map(|f| &p.source == f).unwrap_or(true))
            .collect()
    }

    pub fn spinner_frame(&self) -> char {
        const F: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        F[self.spinner % F.len()]
    }

    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
    }

    pub fn on_results(&mut self, results: Vec<Package>) {
        self.search_results = results;
        self.selected = 0;
        self.marked.clear();
        self.is_loading = false;
        let n = self.visible().len();
        self.status = if n == 0 {
            String::from(" No results found.")
        } else {
            format!(
                " {} packages · ↑↓ navigate · Space mark · i install · Tab filter",
                n
            )
        };
    }

    pub fn cycle_filter(&mut self) {
        self.filter = match &self.filter {
            None => Some(Source::VertexLinux),
            Some(Source::VertexLinux) => Some(Source::Pacman),
            Some(Source::Pacman) => Some(Source::Aur),
            Some(Source::Aur) => Some(Source::Flatpak),
            Some(Source::Flatpak) => None,
        };
        self.selected = 0;
    }

    pub fn move_down(&mut self) {
        let n = self.visible().len();
        if n > 0 {
            self.selected = (self.selected + 1).min(n - 1);
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn toggle_mark(&mut self) {
        if self.marked.contains(&self.selected) {
            self.marked.remove(&self.selected);
        } else {
            self.marked.insert(self.selected);
        }
    }

    pub fn resolve_install(&mut self) {
        let vis = self.visible();
        if vis.is_empty() {
            return;
        }
        let targets: Vec<Package> = if self.marked.is_empty() {
            vis.get(self.selected)
                .map(|p| vec![(*p).clone()])
                .unwrap_or_default()
        } else {
            self.marked
                .iter()
                .filter_map(|&i| vis.get(i).map(|p| (*p).clone()))
                .collect()
        };
        if !targets.is_empty() {
            self.packages_to_install = Some(targets);
        }
    }

    pub fn clear_search(&mut self) {
        self.search_input.clear();
        self.search_results.clear();
        self.marked.clear();
        self.selected = 0;
        self.is_loading = false;
        self.status = String::from(" Type to search · Tab: filter · ? help · q quit");
    }
}
