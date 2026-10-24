mod app;
mod store;
mod style;
mod update;

fn main() -> eframe::Result {
    let dir = store::data_dir();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("no se pudo crear {}: {err}", dir.display());
        std::process::exit(1);
    }

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 700.0])
        .with_title("notetake")
        .with_app_id("notetake")
        // Los botones de ventana y la barra de título los dibuja la app.
        .with_decorations(false);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "notetake",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, dir)))),
    )
}

/// Icono de ventana y barra de tareas. En Wayland el compositor lo busca por
/// `app_id` en `notetake.desktop`; ver README.
fn app_icon() -> Option<eframe::egui::IconData> {
    let img =
        image::load_from_memory(include_bytes!("../assets/icon.png")).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(eframe::egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}
