mod app;
mod blur;
mod config;
mod pty;
mod terminal;
mod theme;

use app::VertexTerm;
use config::Config;

fn main() {
    let config = Config::load();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Vertex Term")
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([320.0, 200.0])
            // system_borders=true → WM draws borders (decorations=true)
            // system_borders=false → we draw CSD (decorations=false)
            .with_decorations(config.system_borders)
            // Allow compositor transparency so the opacity setting works
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "Vertex Term",
        native_options,
        Box::new(|cc| Ok(Box::new(VertexTerm::new(cc)))),
    )
    .expect("Failed to start Vertex Term");
}
