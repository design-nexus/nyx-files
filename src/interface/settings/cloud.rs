// SPDX-License-Identifier: MIT

//! Cloud storage: the sidebar switch, Online Accounts, and a card per account.
//! Account ownership and authorization live in Settings → Online Accounts.

use gtk::{gio, glib, prelude::*};
use nyx_cloud::{Client, Remote};
use nyx_ui::widgets;

use super::{Group, card, note_row, plain_row, row, switch, untranslated_title};
use crate::ui::preferences::PreferenceManager;

pub(super) fn append(group: &Group) {
    let list = group.section("Cloud Storage");
    switch(
        &list,
        "Show Cloud Storage in sidebar",
        "Show accounts enabled for Storage in Online Accounts.",
        PreferenceManager::cloud_sidebar,
        PreferenceManager::set_cloud_sidebar,
    );
    let open = gtk::Button::with_label(&crate::i18n::tr("Open…"));
    open.connect_clicked(|button| {
        if gio::Subprocess::newv(
            &[
                std::ffi::OsStr::new("nyx-settings"),
                std::ffi::OsStr::new("--section"),
                std::ffi::OsStr::new("online-accounts"),
            ],
            gio::SubprocessFlags::NONE,
        )
        .is_err()
        {
            button.set_label(&crate::i18n::tr("Settings could not be opened."));
        }
    });
    let reload = widgets::icon_button(
        "view-refresh-symbolic",
        &crate::i18n::tr("Refresh connections"),
    );
    reload.remove_css_class("flat");
    let controls = widgets::hbox(6);
    controls.append(&open);
    controls.append(&reload);
    list.append(&row(
        "Online Accounts",
        "Add accounts and choose which ones Files can use for storage.",
        Some(controls.upcast_ref()),
    ));
    let notes = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.append(&notes);
    let accounts = gtk::Box::new(gtk::Orientation::Vertical, 0);
    group.wrapper.append(&accounts);
    refresh(&notes, &accounts);
    let (notes, accounts) = (notes.downgrade(), accounts.downgrade());
    reload.connect_clicked(move |_| {
        if let (Some(notes), Some(accounts)) = (notes.upgrade(), accounts.upgrade()) {
            refresh(&notes, &accounts);
        }
    });
}

fn clear(holder: &gtk::Box) {
    while let Some(child) = holder.first_child() {
        holder.remove(&child);
    }
}

/// Loading, empty and unavailable notes go in the Cloud Storage card, accounts in their own.
fn refresh(notes: &gtk::Box, accounts: &gtk::Box) {
    clear(notes);
    clear(accounts);
    notes.append(&note_row("Loading accounts…"));
    let (notes, accounts) = (notes.downgrade(), accounts.downgrade());
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(|| Client::connect()?.list()).await;
        let (Some(notes), Some(accounts)) = (notes.upgrade(), accounts.upgrade()) else {
            return;
        };
        clear(&notes);
        match result {
            Ok(Ok(remotes)) if remotes.is_empty() => notes.append(&note_row(
                "Add an account in Settings → Online Accounts to connect cloud storage.",
            )),
            Ok(Ok(remotes)) => {
                for remote in remotes {
                    append_account(&accounts, remote);
                }
            }
            _ => notes.append(&note_row(
                "Cloud is unavailable. Local files remain available. Check that Cloud and rclone are installed.",
            )),
        }
    });
}

/// Runs a client request off the interface thread, then shows how it went.
fn request(
    button: &gtk::Button,
    status: &gtk::Label,
    run: impl FnOnce(&Client) -> anyhow::Result<()> + Send + 'static,
    done: &'static str,
    failed: &'static str,
) {
    button.set_sensitive(false);
    let (button, status) = (button.downgrade(), status.downgrade());
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || run(&Client::connect()?)).await;
        if let Some(button) = button.upgrade() {
            button.set_sensitive(true);
        }
        if let Some(status) = status.upgrade() {
            status.set_text(&crate::i18n::tr(if matches!(result, Ok(Ok(()))) {
                done
            } else {
                failed
            }));
            status.set_visible(true);
        }
    });
}

fn append_account(holder: &gtk::Box, remote: Remote) {
    let list = card(holder, Some(untranslated_title(&remote.name)));
    let controls = widgets::hbox(6);
    let status = widgets::label("", "settings-option-description");
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.set_visible(false);
    for (label, action) in [("Connect", 0), ("Disconnect", 1), ("Test connection", 2)] {
        let button = gtk::Button::with_label(&crate::i18n::tr(label));
        if action == 0 && !remote.storage_enabled {
            button.set_sensitive(false);
        }
        let (id, output) = (remote.id.clone(), status.clone());
        button.connect_clicked(move |button| {
            let id = id.clone();
            request(
                button,
                &output,
                move |client| {
                    match action {
                        0 => client.mount(&id),
                        1 => client.unmount(&id),
                        _ => client.test(&id),
                    }
                    .map(drop)
                },
                "Operation requested. Refresh connections for current status.",
                "Operation failed. Check the account in Online Accounts.",
            );
        });
        controls.append(&button);
    }
    let mut state = format!(
        "{} · {} · {}",
        remote.provider,
        remote.connection.replace('_', " "),
        remote.mount_status
    );
    if !remote.storage_enabled {
        state.push('\n');
        state.push_str(&crate::i18n::tr(
            "Storage is disabled for Files. Enable it in Online Accounts.",
        ));
    }
    let status_row = plain_row(
        &crate::i18n::tr("Status"),
        &state,
        Some(controls.upcast_ref()),
    );
    if let Some(text) = status_row.first_child().and_downcast::<gtk::Box>() {
        text.append(&status);
    }
    list.append(&status_row);

    let login = gtk::Switch::new();
    login.set_active(remote.preferences.mount_on_login);
    login.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
        "Mount on login",
    ))]);
    list.append(&row(
        "Mount on login",
        "Connect automatically when you sign in.",
        Some(login.upcast_ref()),
    ));
    let cache = entry("Cache target", &remote.preferences.cache_mib.to_string());
    list.append(&row(
        "Cache target",
        "A soft limit, in MiB.",
        Some(cache.upcast_ref()),
    ));
    let bandwidth = entry("Bandwidth limit", &remote.preferences.bandwidth);
    list.append(&row(
        "Bandwidth limit",
        "Leave blank for no limit.",
        Some(bandwidth.upcast_ref()),
    ));
    let save = gtk::Button::with_label(&crate::i18n::tr("Save"));
    let output = status.clone();
    save.connect_clicked(move |button| {
        let cache_mib = match cache.text().trim().parse::<u64>() {
            Ok(size) if size > 0 => size,
            _ => {
                output.set_text(&crate::i18n::tr("Enter a valid cache size."));
                output.set_visible(true);
                return;
            }
        };
        let preferences = nyx_cloud::Preferences {
            mount_on_login: login.is_active(),
            cache_mib,
            bandwidth: bandwidth.text().to_string(),
        };
        let (id, name) = (remote.id.clone(), remote.name.clone());
        request(
            button,
            &output,
            move |client| client.update(&id, &name, &preferences),
            "Connection preferences saved.",
            "Preferences could not be saved.",
        );
    });
    list.append(&row(
        "Connection preferences",
        "Cache and bandwidth changes apply on the next connection. Pending uploads are retained.",
        Some(save.upcast_ref()),
    ));
}

fn entry(label: &str, value: &str) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.set_text(value);
    entry.set_width_chars(10);
    entry.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(label))]);
    entry
}
