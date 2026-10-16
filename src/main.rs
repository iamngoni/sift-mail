//! SiftMail executable: process wiring only. Everything testable lives in the
//! `sift_mail` library; this file is excluded from the coverage gate.

// Release builds on Windows are GUI apps with no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::rc::Rc;
use std::sync::Arc;

use sift_mail::sync::secrets::Keychain;
use sift_mail::sync::service::SyncService;

fn main() {
    let store = match sift_mail::ui::open_default_store() {
        Ok(store) => Rc::new(store),
        Err(error) => {
            eprintln!(
                "{}: could not open the mail store: {error}",
                sift_mail::brand::PRODUCT_NAME
            );
            std::process::exit(1);
        }
    };
    gpui_kit::application()
        .with_assets(sift_mail::assets::SiftAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            sift_mail::ui::quit_when_last_window_closes(cx);
            let sync = store
                .path()
                .map(|path| SyncService::networked(path.to_path_buf(), Arc::new(Keychain)));
            match sift_mail::ui::open_main_window(store, sync, cx) {
                Ok(view) => {
                    view.update(cx, |app, _| {
                        app.use_notifier(Some(sift_mail::ui::system_notifier()));
                    });
                    if let Ok(screen) = std::env::var(sift_mail::ui::OPEN_ENV) {
                        sift_mail::ui::open_start_screen(&view, &screen, cx);
                    }
                }
                Err(error) => {
                    eprintln!(
                        "{}: could not open the main window: {error:#}",
                        sift_mail::brand::PRODUCT_NAME
                    );
                    cx.quit();
                }
            }
        });
}
