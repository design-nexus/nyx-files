// SPDX-License-Identifier: MIT

//! What file operations report: a progress card with Cancel, results and
//! failures, the refreshes the browser asks for, the bookkeeping that keeps
//! folder colours and views with their items, and left-over review.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};
use nyx_ui::{toast, widgets};

use crate::{
    app::BrowserEvent,
    model::{FileEntry, Location},
    services::ArchiveFormat,
    ui::{
        browser::{customization, leftovers, progress, transfer_report},
        preferences::PreferenceManager,
    },
};

use super::{tab::Tab, window::Window};

/// A bottom-right card for the operation running.
pub(super) struct Progress {
    card: gtk::Box,
    title: gtk::Label,
    detail: gtk::Label,
    bar: gtk::ProgressBar,
    cancel: gtk::Button,
    cancel_with: RefCell<Option<Rc<crate::app::Browser>>>,
    /// Entries a trash is moving, offered for permanent deletion if Trash refuses them.
    pub(super) deleting: RefCell<Vec<FileEntry>>,
    /// An archive to extract again with a password, and where to.
    pub(super) extracting: RefCell<Option<(FileEntry, Location)>>,
    /// Where finished extraction goes to show its result.
    pub(super) reveal_after: RefCell<Option<Location>>,
}

impl Progress {
    pub(super) fn new(overlay: &gtk::Overlay) -> Rc<Self> {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 6);
        card.add_css_class("toast");
        card.add_css_class("nyx-toast");
        card.add_css_class("operation-card");
        card.set_halign(gtk::Align::End);
        card.set_valign(gtk::Align::End);
        card.set_margin_end(16);
        card.set_margin_bottom(40);
        card.set_size_request(320, -1);
        let head = widgets::hbox(8);
        let title = widgets::label("", "operation-title");
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        head.append(&title);
        let cancel = widgets::icon_button("process-stop-symbolic", &crate::i18n::tr("Cancel"));
        head.append(&cancel);
        card.append(&head);
        let bar = gtk::ProgressBar::new();
        card.append(&bar);
        let detail = widgets::label("", "dim");
        detail.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        card.append(&detail);
        card.set_visible(false);
        overlay.add_overlay(&card);
        let progress = Rc::new(Progress {
            card,
            title,
            detail,
            bar,
            cancel,
            cancel_with: RefCell::default(),
            deleting: RefCell::default(),
            extracting: RefCell::default(),
            reveal_after: RefCell::default(),
        });
        let weak = Rc::downgrade(&progress);
        progress.cancel.connect_clicked(move |_| {
            if let Some(browser) = weak.upgrade().and_then(|p| p.cancel_with.borrow().clone()) {
                browser.cancel_file_operation();
            }
        });
        progress
    }

    fn start(&self, browser: &Rc<crate::app::Browser>, title: &str, total: usize) {
        self.title.set_text(&format!(
            "{} · {}",
            crate::i18n::tr(title),
            crate::i18n::count("items", total)
        ));
        self.detail.set_text("");
        self.bar.set_fraction(0.0);
        self.cancel_with.replace(Some(browser.clone()));
        self.card.set_visible(true);
    }

    fn update(&self, fraction: f64, detail: Option<&str>) {
        self.bar.set_fraction(fraction.clamp(0.0, 1.0));
        if let Some(detail) = detail {
            self.detail.set_text(detail);
        }
    }

    fn finish(&self) {
        self.cancel_with.take();
        self.card.set_visible(false);
    }
}

type DialogAction<'a> = Option<(&'a str, Box<dyn Fn()>)>;

fn message_dialog(title: &str, text: &str, action: DialogAction<'_>) {
    let (dialog, card) = widgets::dialog(title, 440);
    let body = widgets::label(text, "dim");
    body.set_wrap(true);
    body.set_selectable(true);
    card.append(&body);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    buttons.append(&close);
    if let Some((label, act)) = action {
        let button = gtk::Button::with_label(label);
        button.add_css_class("destructive-action");
        let d = dialog.clone();
        button.connect_clicked(move |_| {
            d.close();
            act();
        });
        buttons.append(&button);
    }
    card.append(&buttons);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    dialog.present();
    close.grab_focus();
}

/// Handles what `tab`'s browser reports about operations and items.
pub(super) fn handle(window: &Rc<Window>, tab: &Rc<Tab>, event: &BrowserEvent) {
    let progress = &window.progress;
    let browser = &tab.browser;
    match event {
        BrowserEvent::TransferStarted { total, moving } => {
            progress.start(browser, if *moving { "Moving" } else { "Copying" }, *total);
        }
        BrowserEvent::TransferProgress {
            completed_items,
            total_files,
            current_file,
            transferred_bytes,
            total_bytes,
            ..
        } => {
            let fraction = match total_bytes {
                Some(total) if *total > 0 => *transferred_bytes as f64 / *total as f64,
                _ => total_files.map_or(0.0, |t| {
                    if t == 0 {
                        0.0
                    } else {
                        *completed_items as f64 / t as f64
                    }
                }),
            };
            let detail = current_file.as_ref().map(|f| f.to_string());
            progress.update(fraction, detail.as_deref());
        }
        BrowserEvent::FlushingToDevice => {
            progress.update(1.0, Some(&crate::i18n::tr("Writing to device…")))
        }
        BrowserEvent::TransferCancellationPending => toast::show(
            &window.overlay,
            &crate::i18n::tr(
                "The device may still be writing. Wait for the transfer to finish or fail before unplugging.",
            ),
        ),
        BrowserEvent::TransferFinished { moved_locations } => {
            if !moved_locations.is_empty() {
                super::actions::finish_cut(moved_locations);
            }
            progress.finish();
        }
        BrowserEvent::TransferCompleted | BrowserEvent::RestorationFinished { .. } => {
            progress.finish()
        }
        BrowserEvent::ItemsReported { report } => {
            progress.finish();
            let text = transfer_report::outcome_detail(String::new(), report);
            if !text.trim().is_empty() {
                message_dialog(
                    &crate::i18n::tr("Transfer finished with notes"),
                    text.trim(),
                    None,
                );
            }
        }
        BrowserEvent::TransferReveal {
            destination,
            locations,
        } => {
            if tab.location().as_ref() == Some(destination) {
                browser.reveal_locations(destination.clone(), locations.clone());
            }
        }
        BrowserEvent::DeletionStarted { total } => progress.start(browser, "Deleting", *total),
        BrowserEvent::RestorationStarted { total } => progress.start(browser, "Restoring", *total),
        BrowserEvent::ArchiveStarted { total } => {
            progress.start(browser, "Processing archive", *total)
        }
        BrowserEvent::DeletionProgress { completed, total }
        | BrowserEvent::RestorationProgress { completed, total }
        | BrowserEvent::ArchiveProgress { completed, total } => {
            progress.update(
                if *total == 0 {
                    0.0
                } else {
                    *completed as f64 / *total as f64
                },
                None,
            );
        }
        BrowserEvent::DeletionFinished { succeeded } => {
            progress.finish();
            if *succeeded {
                progress.deleting.borrow_mut().clear();
            }
        }
        BrowserEvent::ArchiveCompleted { select_name } => {
            progress.finish();
            progress.extracting.take();
            let destination = progress.reveal_after.take().or_else(|| tab.location());
            if let Some(destination) = destination
                && !select_name.is_empty()
                && let Some(target) = destination.child(std::ffi::OsStr::new(select_name))
            {
                browser.reveal_locations(destination, vec![target]);
            }
        }
        BrowserEvent::OperationFailed {
            message,
            password_failure,
            report,
        } => {
            progress.finish();
            let retry = progress.extracting.take();
            if let (Some((entry, destination)), Some(_)) = (retry, password_failure) {
                ask_archive_password(tab, entry, destination);
                return;
            }
            let text = transfer_report::outcome_detail(message.clone(), report);
            message_dialog(
                &crate::i18n::tr("Unable to complete operation"),
                &text,
                None,
            );
        }
        BrowserEvent::OperationCompletedWithErrors {
            message,
            retryable_locations,
            report,
            ..
        } => {
            progress.finish();
            let text = transfer_report::outcome_detail(message.clone(), report);
            let retry: Vec<FileEntry> = progress
                .deleting
                .take()
                .into_iter()
                .filter(|e| retryable_locations.contains(&e.location))
                .collect();
            if retry.is_empty() {
                message_dialog(
                    &crate::i18n::tr("Some items could not be processed"),
                    &text,
                    None,
                );
            } else {
                let b = browser.clone();
                message_dialog(
                    &crate::i18n::tr("Some items can't go to Trash"),
                    &text,
                    Some((
                        &crate::i18n::tr("Delete permanently"),
                        Box::new(move || b.delete(retry.clone(), true)),
                    )),
                );
            }
        }
        BrowserEvent::OperationCancelled {
            completed,
            failed,
            not_attempted,
            affected_locations,
            report,
        } => {
            progress.finish();
            browser.refresh_after_cancellation(affected_locations);
            let summary =
                progress::cancelled_operation_summary(*completed, *failed, *not_attempted);
            message_dialog(
                &crate::i18n::tr("Operation cancelled"),
                &transfer_report::outcome_detail(summary, report),
                None,
            );
        }
        BrowserEvent::OperationRefreshRequired { depths } => {
            progress.finish();
            browser.refresh_operation_columns(depths);
        }
        BrowserEvent::EntryCreated { location } => {
            let group = window.pending_group.borrow().clone();
            if let Some((folder, sources)) = group
                && &folder == location
            {
                window.pending_group.take();
                browser.expect_group_folder(folder.clone());
                super::actions::transfer(window, tab, folder, sources, true);
            }
        }
        BrowserEvent::RenameFailed { message, .. } => toast::show(&window.overlay, message),
        BrowserEvent::ColumnNotResponding {
            not_responding: true,
            ..
        } => {
            toast::show(
                &window.overlay,
                &crate::i18n::tr("This folder isn't responding. Files keeps waiting for it."),
            );
        }
        BrowserEvent::LocationRevealFailed { location } => toast::show(
            &window.overlay,
            &rust_i18n::t!(
                "%{path} is not available in the loaded folder.",
                path = location.display_path()
            ),
        ),
        BrowserEvent::OpenRequested { location } => {
            crate::ui::browser::desktop::open_location_at(location, None, &window.window, browser);
        }
        BrowserEvent::ExtractRequested { entry } => extract(window, tab, entry.clone(), None),
        BrowserEvent::EmptyTrashRequested => super::actions::empty_trash(window),
        BrowserEvent::FolderSortChosen {
            location,
            sort_key,
            sort_direction,
        } => {
            PreferenceManager::shared().set_folder_sort(location, *sort_key, *sort_direction);
        }
        BrowserEvent::ItemRelocated { from, to, merged } => {
            customization::relocate_item_customizations(from, to, *merged);
            PreferenceManager::shared().relocate_folder_views(from, to, *merged);
        }
        BrowserEvent::ItemRemoved {
            location,
            trash_identity,
        } => {
            customization::forget_item_customizations(location, *trash_identity);
            PreferenceManager::shared().forget_folder_views_within(location, *trash_identity);
        }
        BrowserEvent::ItemRestored { location, identity } => {
            customization::restore_item_customizations(location, *identity);
            PreferenceManager::shared().restore_folder_views(location, *identity);
        }
        BrowserEvent::LeftoverReviewRequested { depth } => review_leftovers(tab, *depth),
        BrowserEvent::LeftoversChanged {
            depth,
            announce: true,
        } if !browser.column_leftovers(*depth).is_empty() => {
            let (t, depth) = (Rc::downgrade(tab), *depth);
            toast::show_action(
                &window.overlay,
                &leftovers::title(),
                &crate::i18n::tr("Review"),
                move || {
                    if let Some(tab) = t.upgrade() {
                        review_leftovers(&tab, depth);
                    }
                },
            );
        }
        _ => {}
    }
}

/// Extracts an archive next to itself, or into `into`.
pub(super) fn extract(
    window: &Rc<Window>,
    tab: &Rc<Tab>,
    entry: FileEntry,
    into: Option<Location>,
) {
    let Some(destination) = into.clone().or_else(|| entry.location.parent()) else {
        return;
    };
    if ArchiveFormat::from_extension(&entry.display_name).is_some_and(|f| f.supports_password()) {
        window
            .progress
            .extracting
            .replace(Some((entry.clone(), destination.clone())));
    }
    if into.is_some() {
        window
            .progress
            .reveal_after
            .replace(Some(destination.clone()));
    }
    tab.browser.extract(entry, destination, false, None);
}

fn ask_archive_password(tab: &Rc<Tab>, entry: FileEntry, destination: Location) {
    let tab = tab.clone();
    let extra = widgets::ask_text(
        &crate::i18n::tr("Archive password"),
        &rust_i18n::t!(
            "“%{name}” is protected by a password.",
            name = entry.display_name.clone()
        ),
        "",
        &crate::i18n::tr("Extract"),
        move |password| {
            tab.browser
                .extract(entry.clone(), destination.clone(), false, Some(password))
        },
    );
    if let Some(entry) = extra.prev_sibling().and_downcast::<gtk::Entry>() {
        entry.set_visibility(false);
        entry.set_input_purpose(gtk::InputPurpose::Password);
    }
}

/// Compress: an archive name, a format and an optional password.
pub(super) fn compress(window: &Rc<Window>, tab: &Rc<Tab>, entries: Vec<FileEntry>) {
    if entries.is_empty() || entries.iter().any(|e| e.location.native_path().is_none()) {
        return;
    }
    let Some(destination) = entries[0].location.parent().or_else(|| tab.location()) else {
        return;
    };
    let default = if entries.len() == 1 {
        entries[0].display_name.clone()
    } else {
        "archive".to_owned()
    };
    let (dialog, card) = widgets::dialog(
        &rust_i18n::t!(
            "Compress %{value1}",
            value1 = crate::i18n::count("items", entries.len())
        ),
        440,
    );
    let name = gtk::Entry::new();
    name.set_text(&default);
    card.append(&name);
    let formats = [
        ArchiveFormat::Zip,
        ArchiveFormat::SevenZ,
        ArchiveFormat::TarGz,
        ArchiveFormat::Tar,
    ];
    let chosen = Rc::new(std::cell::Cell::new(ArchiveFormat::Zip));
    let options: Vec<(String, String)> = formats
        .iter()
        .map(|f| (f.extension().to_string(), format!(".{}", f.extension())))
        .collect();
    let password = gtk::PasswordEntry::new();
    password.set_show_peek_icon(true);
    password.set_placeholder_text(Some(&crate::i18n::tr("Password (optional)")));
    let c = chosen.clone();
    let p = password.clone();
    let switch = widgets::segmented(&options, "zip", move |id| {
        if let Some(format) = formats.iter().find(|f| f.extension() == id) {
            c.set(*format);
            p.set_sensitive(format.supports_password());
        }
    });
    card.append(&widgets::row(
        &crate::i18n::tr("Format"),
        "",
        Some(switch.upcast_ref()),
    ));
    card.append(&password);
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let ok = gtk::Button::with_label(&crate::i18n::tr("Compress"));
    ok.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&ok);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let (d, t, w) = (dialog.clone(), tab.clone(), window.clone());
    ok.connect_clicked(move |_| {
        let archive = name.text().trim().to_string();
        if archive.is_empty() {
            name.add_css_class("error");
            return;
        }
        let format = chosen.get();
        let secret = password.text().to_string();
        let secret = (format.supports_password() && !secret.is_empty()).then_some(secret);
        d.close();
        w.progress.reveal_after.replace(Some(destination.clone()));
        t.browser.compress(
            entries.clone(),
            destination.clone(),
            archive,
            crate::services::TransferConflict::KeepBoth,
            format,
            secret,
        );
    });
    dialog.present();
    ok.grab_focus();
}

/// Lists what an interrupted operation left in a folder, to restore or trash.
fn review_leftovers(tab: &Rc<Tab>, depth: usize) {
    let browser = tab.browser.clone();
    let items = browser.column_leftovers(depth);
    let Some(folder) = browser.location_at(depth) else {
        return;
    };
    if items.is_empty() {
        return;
    }
    let (dialog, card) = widgets::dialog(&leftovers::title(), 480);
    card.append(&widgets::label(
        &rust_i18n::t!("In “%{folder}”", folder = folder.display_name()),
        "dim",
    ));
    for (entry, kind) in items {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
        row.add_css_class("properties-details");
        row.append(&widgets::label(&leftovers::heading(kind), "search-name"));
        let about = widgets::label(&leftovers::explanation(kind), "dim");
        about.set_wrap(true);
        row.append(&about);
        row.append(&widgets::label(&entry.display_name, "mono"));
        let actions = widgets::hbox(6);
        actions.set_halign(gtk::Align::End);
        if leftovers::restorable(kind, &folder) {
            let restore = gtk::Button::with_label(&crate::i18n::tr("Restore"));
            let (b, e, d) = (browser.clone(), entry.clone(), dialog.clone());
            restore.connect_clicked(move |_| {
                d.close();
                if matches!(
                    kind,
                    crate::model::hidden_names::Leftover::Originals
                        | crate::model::hidden_names::Leftover::MergeOriginals
                ) {
                    b.recover(e.location.clone());
                    return;
                }
                let taken = b
                    .column_snapshot(depth)
                    .and_then(|s| {
                        b.with_entries(depth, 0..s.count, |es| {
                            es.iter()
                                .map(|e| e.display_name.clone())
                                .collect::<Vec<_>>()
                        })
                    })
                    .unwrap_or_default();
                b.rename(e.clone(), leftovers::recovered_name(&taken));
            });
            actions.append(&restore);
        }
        let remove = gtk::Button::with_label(&crate::i18n::tr("Move to Trash"));
        let (b, e, d) = (browser.clone(), entry.clone(), dialog.clone());
        remove.connect_clicked(move |_| {
            d.close();
            b.delete(vec![e.clone()], false);
        });
        actions.append(&remove);
        row.append(&actions);
        card.append(&row);
    }
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    close.set_halign(gtk::Align::End);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    card.append(&close);
    dialog.present();
    let _ = glib::Propagation::Proceed;
}
