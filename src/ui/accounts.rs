//! Background sync from the main window, and naming new accounts.
//!
//! Every sync runs on the background executor with its own store connection;
//! results come back to the UI thread, which then reloads from the store.
//! Adding accounts lives in [`super::add_account`].

use std::time::Duration;

use chrono::Utc;
use gpui_kit::Context;

use super::app::SiftApp;
use crate::domain::{EmailAddress, Provider};

/// How often accounts sync while the app is open.
pub const SYNC_EVERY: Duration = Duration::from_secs(180);

/// "Gmail", "iCloud", or the capitalised first part of the domain.
pub fn default_name(email: &EmailAddress, provider: Provider) -> String {
    match provider {
        Provider::Google => "Gmail".to_owned(),
        Provider::ICloud => "iCloud".to_owned(),
        Provider::Microsoft365 | Provider::Imap => {
            let first = email.domain().split('.').next().unwrap_or_default();
            let mut chars = first.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_else(|| "Mail".to_owned())
        }
    }
}

impl SiftApp {
    /// Whether a sync is running.
    pub fn is_syncing(&self) -> bool {
        self.syncing
    }

    /// The sync status line, if there is one.
    pub fn sync_status(&self) -> Option<&str> {
        self.sync_status.as_deref()
    }

    /// Syncs now, then every [`SYNC_EVERY`] while the window is open.
    pub(crate) fn schedule_sync(&mut self, cx: &mut Context<Self>) {
        if self.sync.is_none() {
            return;
        }
        self.start_sync(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SYNC_EVERY).await;
                if this.update(cx, |app, cx| app.start_sync(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Runs one sync pass in the background unless one is already running.
    pub fn start_sync(&mut self, cx: &mut Context<Self>) {
        let Some(service) = self.sync.clone() else {
            return;
        };
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.sync_status = Some("Syncing…".to_owned());
        let since = self.store.latest_message_id().unwrap_or(i64::MAX);
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { service.sync_all(Utc::now()) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |app, cx| {
                app.syncing = false;
                app.sync_status = Some(match result {
                    Ok(summary) => summary.status_line(),
                    Err(error) => format!("Sync failed: {error}"),
                });
                app.reload();
                app.notify_new_mail(since);
                app.check_in(Utc::now(), cx);
                cx.notify();
            })
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_follow_the_provider() {
        let name = |s: &str, p| default_name(&EmailAddress::parse(s).unwrap(), p);
        assert_eq!(name("you@fastmail.com", Provider::Imap), "Fastmail");
        assert_eq!(name("you@gmail.com", Provider::Google), "Gmail");
        assert_eq!(name("you@icloud.com", Provider::ICloud), "iCloud");
        assert_eq!(name("you@company.com", Provider::Microsoft365), "Company");
    }
}
