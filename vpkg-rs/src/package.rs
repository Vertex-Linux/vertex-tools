use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Pacman,
    Aur,
    Flatpak,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::Pacman => "PKG",
            Source::Aur => "AUR",
            Source::Flatpak => " FP",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Source::Pacman => Color::Rgb(6, 182, 212),
            Source::Aur => Color::Rgb(234, 179, 8),
            Source::Flatpak => Color::Rgb(139, 92, 246),
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            Source::Pacman => "pm",
            Source::Aur => "aur",
            Source::Flatpak => "fp",
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: Source,
    pub installed: bool,
    pub votes: Option<u32>,
    pub popularity: Option<f64>,
    pub url: Option<String>,
    pub app_id: Option<String>,
}

impl Package {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        source: Source,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: description.into(),
            source,
            installed: false,
            votes: None,
            popularity: None,
            url: None,
            app_id: None,
        }
    }

    pub fn install_name(&self) -> &str {
        if self.source == Source::Flatpak {
            self.app_id.as_deref().unwrap_or(&self.name)
        } else {
            &self.name
        }
    }
}
