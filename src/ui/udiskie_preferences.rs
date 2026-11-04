// SPDX-License-Identifier: MIT

use std::{cell::Cell, rc::Rc, time::Duration};

use gtk::{gio, glib, prelude::*};

use crate::{portal_setup, ui::portal_preferences::IntegrationIndicator};

use super::desktop_integration::{SETUP_RUNNING, set_search_available};

#[cfg(test)]
mod tests;

pub(crate) fn settings_row() -> gtk::Box {
    let (row, content) = crate::ui::portal_preferences::integration_row(
        "Unlock encrypted volumes",
        "When you plug in an encrypted drive, open Files with the password prompt instead of udiskie's dialog. On Omarchy this edits ~/.config/udiskie/config.yml and restarts udiskie.",
    );
    let summary = UdiskieIntegrationStatus::new(&content, &row);
    set_search_available(&row, false);
    // Hidden rows are never mapped, so availability cannot wait on map.
    summary.reload(None);
    let poll = Rc::clone(&summary);
    let weak_row = row.downgrade();
    glib::timeout_add_local(Duration::from_secs(2), move || {
        if weak_row.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        poll.set_sensitive(!SETUP_RUNNING.get() && !poll.busy.get());
        if !SETUP_RUNNING.get() {
            poll.reload(None);
        }
        glib::ControlFlow::Continue
    });
    row
}

pub(crate) struct UdiskieIntegrationStatus {
    pub(crate) row: glib::WeakRef<gtk::Box>,
    pub(crate) indicator: IntegrationIndicator,
    pub(crate) message: glib::WeakRef<gtk::Label>,
    pub(crate) use_strata: glib::WeakRef<gtk::Button>,
    pub(crate) restore: glib::WeakRef<gtk::Button>,
    pub(crate) busy: Cell<bool>,
    pub(crate) known: Cell<bool>,
}

impl UdiskieIntegrationStatus {
    pub(crate) fn new(parent: &gtk::Box, row: &gtk::Box) -> Rc<Self> {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        list.set_margin_top(6);
        list.set_margin_bottom(4);
        let indicator = IntegrationIndicator::new(&list, "Files asks for drive passwords");
        let message = gtk::Label::new(None);
        message.set_visible(false);
        message.set_xalign(0.0);
        message.set_wrap(true);
        message.add_css_class("settings-option-description");
        let actions = crate::ui::portal_preferences::integration_actions();
        let restore = gtk::Button::with_label(&crate::i18n::tr("Restore default"));
        restore.set_sensitive(false);
        restore.set_visible(false);
        let use_strata = gtk::Button::with_label(&crate::i18n::tr("Checking status…"));
        use_strata.add_css_class("suggested-action");
        use_strata.set_sensitive(false);
        actions.append(&restore);
        actions.append(&use_strata);
        parent.append(&list);
        parent.append(&message);
        parent.append(&actions);
        let summary = Rc::new(Self {
            row: row.downgrade(),
            indicator,
            message: message.downgrade(),
            use_strata: use_strata.downgrade(),
            restore: restore.downgrade(),
            busy: Cell::new(false),
            known: Cell::new(false),
        });
        let setup = Rc::clone(&summary);
        use_strata.connect_clicked(move |_| setup.apply(true));
        let reset = Rc::clone(&summary);
        restore.connect_clicked(move |_| reset.apply(false));
        summary
    }

    fn set_sensitive(&self, sensitive: bool) {
        for button in [&self.use_strata, &self.restore] {
            if let Some(button) = button.upgrade() {
                button.set_sensitive(sensitive && self.known.get());
            }
        }
    }

    pub(crate) fn message(&self, text: &str, error: bool) {
        if let Some(message) = self.message.upgrade() {
            message.set_text(text);
            message.set_visible(!text.is_empty());
            if error {
                message.add_css_class("error");
            } else {
                message.remove_css_class("error");
            }
        }
    }

    fn reload(self: &Rc<Self>, failure: Option<String>) {
        if self.busy.replace(true) {
            return;
        }
        let summary = Rc::clone(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(portal_setup::udiskie::status).await;
            summary.busy.set(false);
            summary.show_result(match result {
                Ok(result) => result,
                Err(error) => Err(rust_i18n::t!(
                    "Could not check integration status: %{error}",
                    error = format!("{:?}", error)
                )
                .into_owned()),
            });
            if let Some(failure) = failure {
                summary.message(&failure, true);
            }
        });
    }

    pub(crate) fn show_result(&self, result: Result<portal_setup::udiskie::UdiskieStatus, String>) {
        if let Some(button) = self.use_strata.upgrade() {
            button.set_label(&crate::i18n::tr("Use Files"));
        }
        if let Some(button) = self.restore.upgrade() {
            button.set_label(&crate::i18n::tr("Restore default"));
        }
        match result {
            Ok(status) => {
                if let Some(row) = self.row.upgrade() {
                    set_search_available(&row, status.available);
                }
                self.known.set(true);
                self.indicator.update(Some(status.configured));
                if let Some(use_strata) = self.use_strata.upgrade() {
                    use_strata.set_visible(!status.configured);
                }
                if let Some(restore) = self.restore.upgrade() {
                    restore.set_visible(status.has_installation);
                }
                if let Some(message) = self.message.upgrade()
                    && !message.has_css_class("error")
                {
                    message.set_visible(false);
                }
                self.set_sensitive(!SETUP_RUNNING.get());
            }
            Err(error) => {
                self.known.set(false);
                self.indicator.update(None);
                self.set_sensitive(false);
                self.message(
                    &rust_i18n::t!("Integration status unavailable: %{error}", error = error),
                    true,
                );
            }
        }
    }

    fn show_progress(&self, enable: bool) {
        self.set_sensitive(false);
        self.message("", false);
        let action = if enable {
            &self.use_strata
        } else {
            &self.restore
        };
        if let Some(button) = action.upgrade() {
            button.set_label(&crate::i18n::tr(if enable {
                "Using Files…"
            } else {
                "Restoring defaults…"
            }));
        }
    }

    pub(crate) fn apply(self: &Rc<Self>, enable: bool) {
        if self.busy.get() || !self.known.get() {
            return;
        }
        if SETUP_RUNNING.replace(true) {
            self.message(
                &crate::i18n::tr(
                    "Another integration change is running. Try again when it finishes.",
                ),
                true,
            );
            return;
        }
        self.busy.set(true);
        self.show_progress(enable);
        let hold = self
            .use_strata
            .upgrade()
            .and_then(|button| button.root().and_downcast::<gtk::Window>())
            .and_then(|window| window.application())
            .map(|application| application.hold());
        let summary = Rc::clone(self);
        glib::spawn_future_local(async move {
            let _hold = hold;
            let result = gio::spawn_blocking(move || {
                if enable {
                    portal_setup::udiskie::install()?;
                } else {
                    portal_setup::udiskie::uninstall()?;
                }
                Ok::<_, String>(())
            })
            .await;
            SETUP_RUNNING.set(false);
            summary.busy.set(false);
            let failure = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(error) => Some(
                    rust_i18n::t!(
                        "Integration change failed: %{error}",
                        error = format!("{:?}", error)
                    )
                    .into_owned(),
                ),
            };
            summary.reload(failure);
        });
    }
}
