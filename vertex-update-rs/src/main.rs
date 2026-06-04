mod app;
mod colors;
mod constants;
mod net;
mod ui;
mod workers;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Vertex Updater")
            .with_min_inner_size([100.0, 100.0])
            .with_inner_size([860.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Vertex Updater",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
