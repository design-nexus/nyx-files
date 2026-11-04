// SPDX-License-Identifier: MIT

//! Open With: the apps every selected file can open with, recent ones first,
//! as a menu of quick picks and a dialog with all of them.

use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, prelude::*};
use nyx_ui::{menu, toast, widgets};

use crate::{
    model::FileEntry,
    ui::open_with::{self as backend, Applications},
};

use super::window::Window;

fn files_of(entries: &[FileEntry]) -> Vec<gio::File> {
    entries
        .iter()
        .filter_map(|e| match e.location.native_path() {
            Some(path) => Some(gio::File::for_path(path)),
            None => e.location.uri_value().map(gio::File::for_uri),
        })
        .collect()
}

fn launch(window: &Rc<Window>, app: &gio::AppInfo, files: &[gio::File], content_types: &[String]) {
    let context = gtk::prelude::WidgetExt::display(&window.window).app_launch_context();
    if let Err(error) = backend::launch(app, files, content_types, Some(&context)) {
        toast::show(&window.overlay, error.message());
    }
}

/// Opens the dialog for `entries` (or hands them to the portal's chooser).
pub(super) fn show_dialog(window: &Rc<Window>, entries: Vec<FileEntry>) {
    let files = files_of(&entries);
    if files.is_empty() {
        return;
    }
    if backend::uses_portal() {
        backend::launch_with_portal(&window.window, files, true);
        return;
    }
    let window = window.clone();
    glib::MainContext::default().spawn_local(async move {
        match backend::resolve(&files, || true).await {
            Some(Ok(apps)) => dialog(&window, files, apps),
            Some(Err(message)) => toast::show(&window.overlay, &crate::i18n::tr(message)),
            None => {}
        }
    });
}

fn app_row(app: &gio::AppInfo, default: bool) -> gtk::ListBoxRow {
    let line = widgets::hbox(10);
    line.add_css_class("search-result");
    let icon = gtk::Image::new();
    icon.set_pixel_size(24);
    if let Some(gicon) = app.icon() {
        icon.set_from_gicon(&gicon);
    }
    line.append(&icon);
    let name = widgets::label(&app.display_name(), "search-name");
    name.set_hexpand(true);
    line.append(&name);
    if default {
        line.append(&widgets::tag(&crate::i18n::tr("Default"), "accent"));
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&line));
    row
}

fn dialog(window: &Rc<Window>, files: Vec<gio::File>, apps: Applications) {
    if let Some(reason) = apps.unavailable_reason() {
        toast::show(&window.overlay, &crate::i18n::tr(reason));
        return;
    }
    let (dialog, card) = widgets::dialog(&crate::i18n::tr("Open With"), 460);
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some(&crate::i18n::tr("Search applications")));
    card.append(&filter);
    let list = gtk::ListBox::new();
    list.add_css_class("search-results");
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(320)
        .child(&list)
        .build();
    card.append(&scroller);
    let mut ordered: Vec<(String, gio::AppInfo)> = Vec::new();
    for (section, group) in [
        ("Recent", &apps.choices.recent),
        ("Recommended", &apps.choices.recommended),
        ("Other applications", &apps.choices.other),
    ] {
        for app in group {
            ordered.push((section.to_string(), app.clone()));
        }
    }
    let shown: Rc<RefCell<Vec<gio::AppInfo>>> = Rc::default();
    let render = {
        let (list, shown, filter) = (list.clone(), shown.clone(), filter.clone());
        let default = apps.default.clone();
        move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            shown.borrow_mut().clear();
            let query = filter.text().to_lowercase();
            let mut section = String::new();
            for (group, app) in &ordered {
                if !query.is_empty() && !app.display_name().to_lowercase().contains(&query) {
                    continue;
                }
                if *group != section {
                    section = group.clone();
                    let heading = gtk::ListBoxRow::new();
                    heading.set_selectable(false);
                    heading.set_activatable(false);
                    heading.set_child(Some(&widgets::label(
                        &crate::i18n::tr(group).to_uppercase(),
                        "menu-heading",
                    )));
                    list.append(&heading);
                    shown.borrow_mut().push(app.clone());
                }
                list.append(&app_row(
                    app,
                    default.as_ref().is_some_and(|d| d.equal(app)),
                ));
                shown.borrow_mut().push(app.clone());
            }
        }
    };
    render();
    let r = render.clone();
    filter.connect_search_changed(move |_| r());
    let content_types = apps.content_types.clone();
    let (d, w) = (dialog.clone(), window.clone());
    list.connect_row_activated(move |_, row| {
        if !row.is_activatable() {
            return;
        }
        let Some(app) = usize::try_from(row.index())
            .ok()
            .and_then(|i| shown.borrow().get(i).cloned())
        else {
            return;
        };
        d.close();
        launch(&w, &app, &files, &content_types);
    });
    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    buttons.append(&cancel);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    dialog.present();
    filter.grab_focus();
}

/// The "Open with" submenu: the default and recent or recommended apps for
/// one file, then the dialog with every choice.
pub(super) fn submenu(window: &Rc<Window>, entries: &[FileEntry]) -> menu::Entry {
    let mut items = Vec::new();
    if let [entry] = entries
        && !backend::uses_portal()
    {
        let (content_type, _) = gio::content_type_guess(Some(&entry.native_name), None::<&[u8]>);
        let default = gio::AppInfo::default_for_type(&content_type, false);
        let mut apps: Vec<gio::AppInfo> = default.iter().cloned().collect();
        for app in gio::AppInfo::recommended_for_type(&content_type) {
            if apps.len() >= 6 {
                break;
            }
            if !apps.iter().any(|a| a.equal(&app)) {
                apps.push(app);
            }
        }
        let files = files_of(entries);
        for app in apps {
            let (w, f, t) = (
                window.clone(),
                files.clone(),
                vec![content_type.to_string()],
            );
            let a = app.clone();
            items.push(menu::item(&app.display_name(), move || {
                launch(&w, &a, &f, &t)
            }));
        }
        if !items.is_empty() {
            items.push(menu::separator());
        }
    }
    let (w, e) = (window.clone(), entries.to_vec());
    items.push(menu::item(
        &crate::i18n::tr("Other application…"),
        move || show_dialog(&w, e.clone()),
    ));
    menu::sub(&crate::i18n::tr("Open with"), items).icon("application-x-executable-symbolic")
}
