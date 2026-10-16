//! SiftMail executable: process wiring only. Everything testable lives in the
//! `sift_mail` library; this file is excluded from the coverage gate.

// Release builds on Windows are GUI apps with no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            sift_mail::ui::quit_when_last_window_closes(cx);
            if let Err(error) = sift_mail::ui::open_main_window(cx) {
                eprintln!(
                    "{}: could not open the main window: {error:#}",
                    sift_mail::brand::PRODUCT_NAME
                );
                cx.quit();
            }
        });
}
