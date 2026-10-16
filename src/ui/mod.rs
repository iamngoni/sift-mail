//! Windows and views. [`open_main_window`] is the only way the app opens UI,
//! so tests exercise exactly what users see.

mod accounts;
mod add_account;
mod add_account_view;
mod ai_ui;
mod ai_view;
mod app;
mod chrome;
mod chrome_view;
mod compose;
mod compose_form;
mod onboarding;
mod onboarding_view;
mod render;
mod settings;
mod settings_view;

#[cfg(test)]
mod add_account_tests;
#[cfg(test)]
mod ai_tests;
#[cfg(test)]
mod chrome_tests;
#[cfg(test)]
mod onboarding_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod tests;

pub use app::SiftApp;

use std::rc::Rc;

use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext as _, Bounds, Result, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};

use crate::brand;
use crate::store::{MailStore, StoreResult};
use crate::sync::service::SyncService;
use crate::theme::{self, Appearance};

/// Default window size on first launch, in logical pixels.
pub const DEFAULT_SIZE: (f32, f32) = (1280.0, 820.0);
/// Smallest size the layout is designed for.
pub const MIN_SIZE: (f32, f32) = (720.0, 480.0);

/// Window options for the main window: our own title bar, the formal product
/// name as the OS window title, and the designed default and minimum sizes.
pub fn main_window_options(cx: &App) -> WindowOptions {
    let base = TitleBar::window_options();
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(brand::PRODUCT_NAME.into()),
            ..TitleBar::title_bar_options()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)),
            cx,
        ))),
        window_min_size: Some(size(px(MIN_SIZE.0), px(MIN_SIZE.1))),
        app_id: Some(brand::EXECUTABLE_NAME.into()),
        ..base
    }
}

/// Environment variable that points the app at a different database file,
/// so development and demo runs never touch real mail.
pub const DB_PATH_ENV: &str = "SIFT_MAIL_DB";

/// Opens the store at `SIFT_MAIL_DB` if set, else the default location. With
/// the `demo` feature, an empty store is filled with sample mail so the inbox
/// can be tried without an account.
pub fn open_default_store() -> StoreResult<MailStore> {
    let path = std::env::var_os(DB_PATH_ENV)
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(MailStore::default_path)
        .unwrap_or_else(|| "sift-mail.db".into());
    let store = MailStore::open(&path)?;
    #[cfg(feature = "demo")]
    crate::store::demo::seed(&store, chrono::Utc::now())?;
    Ok(store)
}

/// Installs the theme and opens the main window over `store`. Call once,
/// after `gpui_kit::init`.
pub fn open_main_window(
    store: Rc<MailStore>,
    sync: Option<SyncService>,
    cx: &mut App,
) -> Result<gpui_kit::Entity<SiftApp>> {
    theme::install(Appearance::System, cx)?;
    let options = main_window_options(cx);
    let (_, view) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| SiftApp::new(store, sync, window, cx))
    })?;
    Ok(view)
}

/// Quits when the last window closes. GPUI keeps running without one on
/// Windows and Linux otherwise, which leaves an invisible process behind.
pub fn quit_when_last_window_closes(cx: &mut App) {
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}
