// SPDX-License-Identifier: MIT

//! What file operations report: a progress card with Cancel, results and
//! failures, the refreshes the browser asks for, the bookkeeping that keeps
//! folder colours and views with their items, and left-over review.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{glib, prelude::*};
use nyx_ui::{toast, widgets};

use crate::{
    app::{Browser, BrowserEvent},
    model::{FileEntry, Location},
    services::{ArchiveFormat, ListenerGuard},
    ui::{
        browser::{customization, leftovers, progress, transfer_report},
        file_operations::{self, FileOperationId, FileOperationStatus, OperationOwner},
        preferences::PreferenceManager,
    },
};

use super::{tab::Tab, window::Window};

#[cfg(test)]
mod tests;

/// A bottom-right card for the operation running in front of this window.
/// Every operation, in front or in the background, is also listed in Jobs.
pub(super) struct Progress {
    /// Holds the card: toasts replace any `toast` child of the overlay, and
    /// the card is styled as one.
    holder: gtk::Box,
    title: gtk::Label,
    detail: gtk::Label,
    bar: gtk::ProgressBar,
    cancel: gtk::Button,
    background: gtk::Button,
    /// Operations this window started in front, oldest first; the card shows
    /// the newest still in front.
    fronts: RefCell<Vec<(FileOperationId, Weak<Browser>)>>,
    shown: Cell<Option<FileOperationId>>,
    observer: RefCell<Option<ListenerGuard<file_operations::Callback>>>,
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
        card.set_size_request(320, -1);
        let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
        holder.set_halign(gtk::Align::End);
        holder.set_valign(gtk::Align::End);
        holder.set_margin_end(16);
        holder.set_margin_bottom(40);
        holder.append(&card);
        let head = widgets::hbox(8);
        let title = widgets::label("", "operation-title");
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        head.append(&title);
        let background =
            widgets::icon_button("go-bottom-symbolic", &crate::i18n::tr("Run in background"));
        head.append(&background);
        let cancel = widgets::icon_button("process-stop-symbolic", &crate::i18n::tr("Cancel"));
        head.append(&cancel);
        card.append(&head);
        let bar = gtk::ProgressBar::new();
        card.append(&bar);
        let detail = widgets::label("", "dim");
        detail.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        card.append(&detail);
        holder.set_visible(false);
        overlay.add_overlay(&holder);
        let progress = Rc::new(Progress {
            holder,
            title,
            detail,
            bar,
            cancel,
            background,
            fronts: RefCell::default(),
            shown: Cell::default(),
            observer: RefCell::default(),
            deleting: RefCell::default(),
            extracting: RefCell::default(),
            reveal_after: RefCell::default(),
        });
        let weak = Rc::downgrade(&progress);
        progress.cancel.connect_clicked(move |_| {
            if let Some(id) = weak.upgrade().and_then(|p| p.shown.get()) {
                file_operations::shared().cancel(id);
            }
        });
        let weak = Rc::downgrade(&progress);
        progress.background.connect_clicked(move |button| {
            if let Some(progress) = weak.upgrade() {
                progress.send_to_background(button);
            }
        });
        let weak = Rc::downgrade(&progress);
        let observer = file_operations::shared().observe(Rc::new(move || {
            if let Some(progress) = weak.upgrade() {
                progress.refresh();
            }
        }));
        progress.observer.replace(Some(observer));
        progress
    }

    /// Shows the operation `browser` has just started in front.
    fn follow_front(&self, browser: &Rc<Browser>) {
        let owner: Rc<dyn OperationOwner> = browser.clone();
        if let Some(id) = file_operations::shared().front_of(&owner)
            && !self.fronts.borrow().iter().any(|(front, _)| *front == id)
        {
            self.fronts.borrow_mut().push((id, Rc::downgrade(browser)));
        }
        self.refresh();
    }

    fn refresh(&self) {
        let operations = file_operations::shared();
        let shown = {
            let mut fronts = self.fronts.borrow_mut();
            fronts.retain(|(id, browser)| {
                browser.strong_count() > 0
                    && operations
                        .snapshot_of(*id)
                        .is_some_and(|s| s.status.is_active() && !s.background)
            });
            fronts.last().cloned()
        };
        let Some((snapshot, browser)) =
            shown.and_then(|(id, browser)| Some((operations.snapshot_of(id)?, browser.upgrade()?)))
        else {
            self.shown.set(None);
            self.holder.set_visible(false);
            return;
        };
        self.shown.set(Some(snapshot.id));
        self.title.set_text(&snapshot.title);
        self.detail.set_text(&snapshot.detail);
        self.bar.set_fraction(snapshot.fraction);
        self.cancel
            .set_sensitive(snapshot.status == FileOperationStatus::Running);
        self.background
            .set_visible(browser.backgroundable_operation() == Some(snapshot.request));
        self.holder.set_visible(true);
    }

    fn send_to_background(&self, button: &gtk::Button) {
        let operations = file_operations::shared();
        let Some(snapshot) = self.shown.get().and_then(|id| operations.snapshot_of(id)) else {
            return;
        };
        let browser = self
            .fronts
            .borrow()
            .iter()
            .find(|(id, _)| *id == snapshot.id)
            .and_then(|(_, browser)| browser.upgrade());
        let Some(browser) = browser else {
            return;
        };
        if !browser.background_file_operation(snapshot.request) {
            return;
        }
        operations.set_background(snapshot.id);
        self.refresh();
        if let Some(overlay) = button
            .ancestor(gtk::Overlay::static_type())
            .and_downcast::<gtk::Overlay>()
        {
            let anchor = overlay.downgrade();
            toast::show_action(
                &overlay,
                &crate::i18n::tr("Running in the background"),
                &crate::i18n::tr("Show"),
                move || {
                    if let Some(anchor) = anchor.upgrade() {
                        crate::ui::jobs::present_all(&anchor);
                    }
                },
            );
        }
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
        BrowserEvent::BackgroundOperation { event, .. } => handle(window, tab, event),
        BrowserEvent::TransferStarted { .. }
        | BrowserEvent::DeletionStarted { .. }
        | BrowserEvent::RestorationStarted { .. }
        | BrowserEvent::ArchiveStarted { .. } => progress.follow_front(browser),
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
        }
        BrowserEvent::ItemsReported { report } => {
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
            // A drop onto a subfolder opens it, from its parent in Columns.
            let parent_depth = (tab.mode() == crate::ui::browser_modes::BrowserMode::Columns
                && browser.open_depth(destination).is_none())
            .then(|| destination.parent().and_then(|p| browser.open_depth(&p)))
            .flatten();
            match parent_depth {
                Some(depth) => {
                    browser.descend_revealing(depth, destination.clone(), locations.clone())
                }
                None => {
                    browser.reveal_locations(destination.clone(), locations.clone());
                }
            }
        }
        BrowserEvent::DeletionFinished { succeeded: true } => {
            progress.deleting.borrow_mut().clear();
        }
        BrowserEvent::ArchiveCompleted { select_name } => {
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
        BrowserEvent::FolderSortChosen {
            location,
            sort_key,
            sort_direction,
        } => {
            PreferenceManager::shared().set_folder_sort(location, *sort_key, *sort_direction);
            window.sync_header();
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

struct LeftoverRow {
    date: gtk::Label,
    inside: gtk::Label,
    restore: Option<gtk::Button>,
}

fn describe_inside(inside: &crate::services::LeftoverInside) -> Option<String> {
    use crate::services::LeftoverInside;
    match inside {
        LeftoverInside::Item => None,
        LeftoverInside::Items { names, more } if names.is_empty() && *more == 0 => {
            Some(crate::i18n::tr("Nothing inside"))
        }
        LeftoverInside::Items { names, more } => {
            let mut lines: Vec<String> = names
                .iter()
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            if *more > 0 {
                lines.push(crate::i18n::count("more_items", *more));
            }
            Some(lines.join("\n"))
        }
        LeftoverInside::StillOpen(reason) | LeftoverInside::Unreadable(reason) => {
            Some(reason.clone())
        }
        LeftoverInside::Gone => Some(crate::i18n::tr("No longer here")),
    }
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
    card.append(&widgets::label(
        &crate::i18n::tr(
            "Another program or computer may still be using these items. Restore never replaces an existing item.",
        ),
        "dim",
    ));
    let mut checked: std::collections::HashMap<std::ffi::OsString, LeftoverRow> =
        std::collections::HashMap::new();
    let names: Vec<std::ffi::OsString> = items.iter().map(|(e, _)| e.native_name.clone()).collect();
    for (entry, kind) in items {
        let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
        row.add_css_class("properties-details");
        row.append(&widgets::label(&leftovers::heading(kind), "search-name"));
        let about = widgets::label(&leftovers::explanation(kind), "dim");
        about.set_wrap(true);
        row.append(&about);
        row.append(&widgets::label(&entry.display_name, "mono"));
        let date = widgets::label("", "dim");
        row.append(&date);
        let inside = widgets::label("", "dim");
        inside.set_wrap(true);
        inside.set_visible(false);
        row.append(&inside);
        let actions = widgets::hbox(6);
        actions.set_halign(gtk::Align::End);
        let mut restore_button = None;
        if leftovers::restorable(kind, &folder) {
            let restore = gtk::Button::with_label(&crate::i18n::tr("Restore"));
            restore_button = Some(restore.clone());
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
        checked.insert(
            entry.native_name.clone(),
            LeftoverRow {
                date,
                inside,
                restore: restore_button,
            },
        );
    }
    // Contents, dates and whether something still has them open arrive in the background.
    let review = browser.review_leftovers(
        &folder,
        names,
        Box::new(move |reviewed| {
            for reviewed in reviewed {
                let Some(row) = checked.get(&reviewed.name) else {
                    continue;
                };
                crate::util::set_full_modified_date(&row.date, reviewed.modified);
                if let Some(text) = describe_inside(&reviewed.inside) {
                    row.inside.set_text(&text);
                    row.inside.set_visible(true);
                }
                if let (
                    Some(restore),
                    crate::services::LeftoverInside::StillOpen(_)
                    | crate::services::LeftoverInside::Gone,
                ) = (&row.restore, &reviewed.inside)
                {
                    restore.set_visible(false);
                }
            }
        }),
    );
    let pending = std::cell::RefCell::new(Some(review));
    dialog.connect_close_request(move |_| {
        pending.take();
        glib::Propagation::Proceed
    });
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    close.set_halign(gtk::Align::End);
    let d = dialog.clone();
    close.connect_clicked(move |_| d.close());
    card.append(&close);
    dialog.present();
    let _ = glib::Propagation::Proceed;
}
