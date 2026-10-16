//! Embedded assets. GPUI Kit ships 101 component icons; the design also uses
//! a handful of other Lucide icons (ISC licence, see `assets/icons/LICENSE-LUCIDE`),
//! embedded here so only what's used ends up in the binary. Everything else
//! falls through to GPUI Kit's own assets.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

pub use gpui_kit::assets::IconName as Lucide;

/// The extra icons, by asset path.
const EXTRA: [(&str, &[u8]); 18] = [
    (
        "icons/cloud.svg",
        include_bytes!("../assets/icons/cloud.svg"),
    ),
    (
        "icons/key-round.svg",
        include_bytes!("../assets/icons/key-round.svg"),
    ),
    ("icons/lock.svg", include_bytes!("../assets/icons/lock.svg")),
    (
        "icons/server.svg",
        include_bytes!("../assets/icons/server.svg"),
    ),
    (
        "icons/shield-check.svg",
        include_bytes!("../assets/icons/shield-check.svg"),
    ),
    (
        "icons/calendar-days.svg",
        include_bytes!("../assets/icons/calendar-days.svg"),
    ),
    (
        "icons/clock.svg",
        include_bytes!("../assets/icons/clock.svg"),
    ),
    (
        "icons/grip-vertical.svg",
        include_bytes!("../assets/icons/grip-vertical.svg"),
    ),
    (
        "icons/keyboard.svg",
        include_bytes!("../assets/icons/keyboard.svg"),
    ),
    (
        "icons/log-in.svg",
        include_bytes!("../assets/icons/log-in.svg"),
    ),
    (
        "icons/mail-open.svg",
        include_bytes!("../assets/icons/mail-open.svg"),
    ),
    ("icons/mail.svg", include_bytes!("../assets/icons/mail.svg")),
    (
        "icons/paperclip.svg",
        include_bytes!("../assets/icons/paperclip.svg"),
    ),
    (
        "icons/pencil.svg",
        include_bytes!("../assets/icons/pencil.svg"),
    ),
    ("icons/send.svg", include_bytes!("../assets/icons/send.svg")),
    (
        "icons/sparkles.svg",
        include_bytes!("../assets/icons/sparkles.svg"),
    ),
    (
        "icons/trash.svg",
        include_bytes!("../assets/icons/trash.svg"),
    ),
    (
        "icons/user-plus.svg",
        include_bytes!("../assets/icons/user-plus.svg"),
    ),
];

/// Sift's asset source: the extra icons, then GPUI Kit's.
pub struct SiftAssets;

impl AssetSource for SiftAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match EXTRA.iter().find(|(p, _)| *p == path) {
            Some((_, bytes)) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = gpui_kit::assets::Assets.list(path)?;
        all.extend(
            EXTRA
                .iter()
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        all.sort();
        all.dedup();
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_icons_load_and_the_rest_fall_through() {
        for (path, _) in EXTRA {
            let bytes = SiftAssets.load(path).unwrap().unwrap();
            assert!(bytes.starts_with(b"<svg"), "{path}");
        }
        assert!(SiftAssets.load("icons/check.svg").unwrap().is_some());
        assert_eq!(SiftAssets.load("").unwrap(), None);
        let icons = SiftAssets.list("icons/").unwrap();
        assert!(icons.iter().any(|i| i == "icons/pencil.svg"));
        assert!(icons.iter().any(|i| i == "icons/check.svg"));
        assert_eq!(Lucide::Pencil.path(), "icons/pencil.svg");
    }
}
