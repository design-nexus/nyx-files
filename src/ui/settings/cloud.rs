// SPDX-License-Identifier: MIT
//! Connection preferences; account ownership and authorization live in Settings.
use super::*;
use nyx_cloud::{Client, Remote};

pub(crate) fn cloud_page(manager: Rc<PreferenceManager>) -> gtk::Widget {
    let content = page_content();
    let group = settings_group(&content, "CLOUD STORAGE");
    let (row, toggle) = settings_option(
        "Show Cloud Storage in sidebar",
        "Show accounts enabled for Storage in Online Accounts.",
        manager.cloud_sidebar(),
    );
    bindings::bind_switch(
        &manager,
        &toggle,
        PreferenceManager::cloud_sidebar,
        PreferenceManager::set_cloud_sidebar,
    );
    group.append(&row);
    let accounts = gtk::Button::with_label(&crate::i18n::tr("Online Accounts…"));
    accounts.add_css_class("settings-action-button");
    search::tag(&accounts, "Online Accounts");
    accounts.connect_clicked(|button| {
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
    group.append(&accounts);
    let holder = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&holder);
    refresh(&holder);
    let button = gtk::Button::with_label(&crate::i18n::tr("Refresh connections"));
    button.add_css_class("settings-action-button");
    search::tag(&button, "Refresh connections");
    let weak = holder.downgrade();
    button.connect_clicked(move |_| {
        if let Some(h) = weak.upgrade() {
            refresh(&h);
        }
    });
    content.append(&button);
    scrollable_page(&content, None)
}
fn message(body: &gtk::Box, text: &str) {
    let label = gtk::Label::new(Some(&crate::i18n::tr(text)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class("settings-option-description");
    body.append(&label);
}
fn refresh(holder: &gtk::Box) {
    while let Some(c) = holder.first_child() {
        holder.remove(&c);
    }
    message(holder, "Loading accounts…");
    let weak = holder.downgrade();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(|| Client::connect()?.list()).await;
        let Some(holder) = weak.upgrade() else { return };
        while let Some(c) = holder.first_child() {
            holder.remove(&c);
        }
        match result {
            Ok(Ok(remotes)) => {
                if remotes.is_empty() {
                    message(
                        &holder,
                        "Add an account in Settings → Online Accounts to connect cloud storage.",
                    );
                }
                for remote in remotes {
                    append_account(&holder, remote);
                }
            }
            _ => message(
                &holder,
                "Cloud is unavailable. Local files remain available. Check that Cloud and rclone are installed.",
            ),
        }
    });
}
fn append_account(body: &gtk::Box, remote: Remote) {
    let group = settings_group(body, &remote.name);
    message(
        &group,
        &format!(
            "{} · {} · {}",
            remote.provider,
            remote.connection.replace('_', " "),
            remote.mount_status
        ),
    );
    if !remote.storage_enabled {
        message(
            &group,
            "Storage is disabled for Files. Enable it in Online Accounts.",
        );
    }
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let status = gtk::Label::new(None);
    status.set_wrap(true);
    status.set_xalign(0.0);
    for (label, action) in [("Connect", 0), ("Disconnect", 1), ("Test connection", 2)] {
        let button = gtk::Button::with_label(&crate::i18n::tr(label));
        button.add_css_class("settings-action-button");
        if action == 0 && !remote.storage_enabled {
            button.set_sensitive(false);
        }
        let id = remote.id.clone();
        let output = status.downgrade();
        button.connect_clicked(move |button| {
            button.set_sensitive(false);
            let weak = button.downgrade();
            let id = id.clone();
            let output = output.clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    let c = Client::connect()?;
                    match action {
                        0 => c.mount(&id),
                        1 => c.unmount(&id),
                        _ => c.test(&id),
                    }
                })
                .await;
                if let Some(b) = weak.upgrade() {
                    b.set_sensitive(true);
                }
                if let Some(s) = output.upgrade() {
                    s.set_text(&crate::i18n::tr(if matches!(result, Ok(Ok(_))) {
                        "Operation requested. Refresh connections for current status."
                    } else {
                        "Operation failed. Check the account in Online Accounts."
                    }));
                }
            });
        });
        controls.append(&button);
    }
    group.append(&controls);
    let (login_row, login) = settings_option(
        "Mount on login",
        "Connect automatically when you sign in.",
        remote.preferences.mount_on_login,
    );
    group.append(&login_row);
    let cache = entry(
        &group,
        "Cache target (MiB, soft limit)",
        &remote.preferences.cache_mib.to_string(),
    );
    let bandwidth = entry(
        &group,
        "Bandwidth limit (blank is unlimited)",
        &remote.preferences.bandwidth,
    );
    message(
        &group,
        "Cache and bandwidth changes apply on the next connection. Pending uploads are retained.",
    );
    let save = gtk::Button::with_label(&crate::i18n::tr("Save connection preferences"));
    save.add_css_class("settings-action-button");
    let output = status.downgrade();
    save.connect_clicked(move |button| {
        let Ok(cache_mib) = cache.text().parse::<u64>() else {
            if let Some(s) = output.upgrade() {
                s.set_text(&crate::i18n::tr("Enter a valid cache size."));
            }
            return;
        };
        if cache_mib == 0 {
            return;
        }
        let prefs = nyx_cloud::Preferences {
            mount_on_login: login.is_active(),
            cache_mib,
            bandwidth: bandwidth.text().to_string(),
        };
        let id = remote.id.clone();
        let name = remote.name.clone();
        let output = output.clone();
        let weak = button.downgrade();
        button.set_sensitive(false);
        glib::spawn_future_local(async move {
            let result =
                gio::spawn_blocking(move || Client::connect()?.update(&id, &name, &prefs)).await;
            if let Some(b) = weak.upgrade() {
                b.set_sensitive(true);
            }
            if let Some(s) = output.upgrade() {
                s.set_text(&crate::i18n::tr(if matches!(result, Ok(Ok(()))) {
                    "Connection preferences saved."
                } else {
                    "Preferences could not be saved."
                }));
            }
        });
    });
    group.append(&save);
    group.append(&status);
}
fn entry(body: &gtk::Box, label: &str, value: &str) -> gtk::Entry {
    message(body, label);
    let e = gtk::Entry::new();
    e.add_css_class("form-control");
    e.set_text(value);
    body.append(&e);
    e
}
