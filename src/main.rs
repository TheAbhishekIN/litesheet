#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    // `litesheet path/to/file.xlsx` opens a file at startup.
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("LiteSheet")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([640.0, 400.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Runs before the event loop exists: early enough to catch the
        // document macOS launched us with (double-click in Finder).
        event_loop_builder: Some(Box::new(|_| litesheet::platform::install())),
        ..Default::default()
    };
    eframe::run_native(
        "LiteSheet",
        options,
        Box::new(move |cc| Ok(Box::new(litesheet::App::new(cc, open)))),
    )
}
