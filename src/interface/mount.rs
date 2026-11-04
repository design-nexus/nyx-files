// SPDX-License-Identifier: MIT

//! Mounting with Nyx dialogs: the password a server or encrypted volume asks
//! for, questions it asks, and "Connect to server".

use std::rc::Rc;

use gtk::{gio, glib, prelude::*};
use nyx_ui::widgets;

use crate::model::Location;

/// A mount operation whose questions are answered in Nyx dialogs.
pub(super) fn operation() -> gio::MountOperation {
    let operation = gio::MountOperation::new();
    operation.connect_ask_password(|op, message, user, domain, flags| {
        ask_password(op, message, user, domain, flags)
    });
    operation.connect_local("ask-question", false, |args| {
        let op = args.first()?.get::<gio::MountOperation>().ok()?;
        let message = args.get(1)?.get::<String>().ok()?;
        let choices = args.get(2)?.get::<glib::StrV>().ok()?;
        let choices: Vec<glib::GString> = choices
            .iter()
            .map(|c| glib::GString::from(c.as_str()))
            .collect();
        ask_question(&op, &message, &choices);
        None
    });
    operation
}

fn ask_password(
    op: &gio::MountOperation,
    message: &str,
    user: &str,
    domain: &str,
    flags: gio::AskPasswordFlags,
) {
    let title = message.lines().next().unwrap_or(message).to_string();
    let (dialog, card) = widgets::dialog(&title, 420);
    let rest: Vec<&str> = message
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !rest.is_empty() {
        let note = widgets::label(&rest.join("\n"), "dim");
        note.set_wrap(true);
        card.append(&note);
    }
    let field = |placeholder: &str, text: &str, visible: bool| {
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some(&crate::i18n::tr(placeholder)));
        entry.set_text(text);
        entry.set_visibility(visible);
        entry.set_input_purpose(if visible {
            gtk::InputPurpose::FreeForm
        } else {
            gtk::InputPurpose::Password
        });
        entry
    };
    let anonymous = flags
        .contains(gio::AskPasswordFlags::ANONYMOUS_SUPPORTED)
        .then(|| {
            let check = gtk::CheckButton::with_label(&crate::i18n::tr("Connect anonymously"));
            card.append(&check);
            check
        });
    let user_entry = flags
        .contains(gio::AskPasswordFlags::NEED_USERNAME)
        .then(|| {
            let e = field("User name", user, true);
            card.append(&e);
            e
        });
    let domain_entry = flags.contains(gio::AskPasswordFlags::NEED_DOMAIN).then(|| {
        let e = field("Domain", domain, true);
        card.append(&e);
        e
    });
    let password_entry = flags
        .contains(gio::AskPasswordFlags::NEED_PASSWORD)
        .then(|| {
            let e = field("Password", "", false);
            card.append(&e);
            e
        });
    let remember = flags
        .contains(gio::AskPasswordFlags::SAVING_SUPPORTED)
        .then(|| {
            widgets::segmented(
                &[
                    ("never".into(), crate::i18n::tr("Forget")),
                    ("session".into(), crate::i18n::tr("Until logout")),
                    ("always".into(), crate::i18n::tr("Remember")),
                ],
                "never",
                {
                    let op = op.clone();
                    move |id| {
                        op.set_password_save(match id.as_str() {
                            "session" => gio::PasswordSave::ForSession,
                            "always" => gio::PasswordSave::Permanently,
                            _ => gio::PasswordSave::Never,
                        })
                    }
                },
            )
        });
    if let Some(remember) = &remember {
        card.append(remember);
    }
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let ok = gtk::Button::with_label(&crate::i18n::tr("Connect"));
    ok.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&ok);
    card.append(&buttons);
    let answered = Rc::new(std::cell::Cell::new(false));
    {
        let (d, op, answered) = (dialog.clone(), op.clone(), answered.clone());
        cancel.connect_clicked(move |_| {
            answered.set(true);
            op.reply(gio::MountOperationResult::Aborted);
            d.close();
        });
    }
    let submit = {
        let (d, op, answered) = (dialog.clone(), op.clone(), answered.clone());
        let (user_entry, domain_entry, password_entry, anonymous) = (
            user_entry.clone(),
            domain_entry.clone(),
            password_entry.clone(),
            anonymous.clone(),
        );
        move || {
            answered.set(true);
            if let Some(a) = &anonymous {
                op.set_anonymous(a.is_active());
            }
            if let Some(e) = &user_entry {
                op.set_username(Some(&e.text()));
            }
            if let Some(e) = &domain_entry {
                op.set_domain(Some(&e.text()));
            }
            if let Some(e) = &password_entry {
                op.set_password(Some(&e.text()));
            }
            op.reply(gio::MountOperationResult::Handled);
            d.close();
        }
    };
    let s = submit.clone();
    ok.connect_clicked(move |_| s());
    for entry in [&user_entry, &domain_entry, &password_entry]
        .into_iter()
        .flatten()
    {
        let s = submit.clone();
        entry.connect_activate(move |_| s());
    }
    {
        let op = op.clone();
        dialog.connect_close_request(move |_| {
            if !answered.replace(true) {
                op.reply(gio::MountOperationResult::Aborted);
            }
            glib::Propagation::Proceed
        });
    }
    dialog.present();
    if let Some(first) = [&user_entry, &password_entry]
        .into_iter()
        .flatten()
        .find(|e| e.text().is_empty())
    {
        first.grab_focus();
    }
}

fn ask_question(op: &gio::MountOperation, message: &str, choices: &[glib::GString]) {
    let title = message.lines().next().unwrap_or(message).to_string();
    let (dialog, card) = widgets::dialog(&title, 420);
    let rest: Vec<&str> = message
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !rest.is_empty() {
        let note = widgets::label(&rest.join("\n"), "dim");
        note.set_wrap(true);
        card.append(&note);
    }
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let answered = Rc::new(std::cell::Cell::new(false));
    for (index, choice) in choices.iter().enumerate() {
        let button = gtk::Button::with_label(choice);
        let (d, op, answered) = (dialog.clone(), op.clone(), answered.clone());
        button.connect_clicked(move |_| {
            answered.set(true);
            op.set_choice(index as i32);
            op.reply(gio::MountOperationResult::Handled);
            d.close();
        });
        buttons.append(&button);
    }
    card.append(&buttons);
    let op = op.clone();
    dialog.connect_close_request(move |_| {
        if !answered.replace(true) {
            op.reply(gio::MountOperationResult::Aborted);
        }
        glib::Propagation::Proceed
    });
    dialog.present();
}

fn file_of(location: &Location) -> gio::File {
    match location.native_path() {
        Some(path) => gio::File::for_path(path),
        None => gio::File::for_uri(location.uri_value().unwrap_or_default()),
    }
}

/// Mounts what `location` lives on; `done` gets an error message, if any,
/// already in words for the user (none when the user cancelled).
pub(super) fn mount_location(
    location: &Location,
    done: impl FnOnce(Result<(), Option<String>>) + 'static,
) {
    let file = file_of(location);
    let op = operation();
    glib::MainContext::default().spawn_local(async move {
        let result = file
            .mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(&op))
            .await;
        done(match result {
            Ok(()) => Ok(()),
            Err(e) if e.matches(gio::IOErrorEnum::AlreadyMounted) => Ok(()),
            Err(e)
                if e.matches(gio::IOErrorEnum::FailedHandled)
                    || e.matches(gio::IOErrorEnum::Cancelled) =>
            {
                Err(None)
            }
            Err(e) => Err(Some(e.message().to_string())),
        });
    });
}

/// "Connect to server": an address, and whether to pin it once it opens.
pub(super) fn connect_dialog(open: impl Fn(Location, bool) + 'static) {
    let (dialog, card) = widgets::dialog(&crate::i18n::tr("Connect to server"), 440);
    let address = gtk::Entry::new();
    address.set_placeholder_text(Some("smb://server/share"));
    card.append(&address);
    let error = widgets::label("", "error-text");
    error.set_wrap(true);
    error.set_visible(false);
    card.append(&error);
    let pin = gtk::CheckButton::with_label(&crate::i18n::tr("Pin to sidebar"));
    card.append(&pin);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let spinner = gtk::Spinner::new();
    buttons.append(&spinner);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let connect = gtk::Button::with_label(&crate::i18n::tr("Connect"));
    connect.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&connect);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let open = Rc::new(open);
    let submit = {
        let (d, address, error, pin, spinner, connect) = (
            dialog.clone(),
            address.clone(),
            error.clone(),
            pin.clone(),
            spinner.clone(),
            connect.clone(),
        );
        move || {
            let text = address.text().trim().to_string();
            if gio::File::for_commandline_arg(&text).uri_scheme().is_none() || !text.contains("://")
            {
                error.set_text(&crate::i18n::tr(
                    "Enter an address such as smb://server/share",
                ));
                error.set_visible(true);
                return;
            }
            error.set_visible(false);
            spinner.start();
            connect.set_sensitive(false);
            let location = Location::uri(text);
            let (d, error, spinner, connect, open, pinned) = (
                d.clone(),
                error.clone(),
                spinner.clone(),
                connect.clone(),
                open.clone(),
                pin.is_active(),
            );
            let target = location.clone();
            mount_location(&location, move |result| {
                spinner.stop();
                connect.set_sensitive(true);
                match result {
                    Ok(()) => {
                        d.close();
                        open(target, pinned);
                    }
                    Err(Some(message)) => {
                        error.set_text(&message);
                        error.set_visible(true);
                    }
                    Err(None) => {}
                }
            });
        }
    };
    let s = submit.clone();
    connect.connect_clicked(move |_| s());
    address.connect_activate(move |_| submit());
    dialog.present();
    address.grab_focus();
}
