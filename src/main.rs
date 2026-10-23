mod app;
mod store;

fn main() -> eframe::Result {
    let dir = store::data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("no se pudo crear {}: {err}", dir.display());
        std::process::exit(1);
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0])
            .with_title("notetake"),
        ..Default::default()
    };

    eframe::run_native(
        "notetake",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, dir)))),
    )
}
