use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px};
use std::fs;

mod components;
mod models;
mod util;

use components::NoteApp;
use util::get_db_path;

fn main() {
    // Print database path to help with debugging
    let db_path = get_db_path();
    println!("Using database at: {:?}", db_path);

    // Check if database exists
    if db_path.exists() {
        println!("Database file exists");
        if let Ok(metadata) = fs::metadata(&db_path) {
            println!("Database size: {} bytes", metadata.len());
        }
    } else {
        println!("Database file does not exist yet, will be created when app starts");
    }

    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, gpui::size(px(1000.0), px(710.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| NoteApp::new(cx)),
        )
        .unwrap();

        cx.activate(true);
    });
}
