// SPDX-License-Identifier: MIT

//! Open With: the apps every selected file can open with, recent ones first,
//! as a menu of quick picks and a dialog with all of them.

use std::rc::Rc;

use gtk::{gio, glib, prelude::*};
use nyx_ui::{menu, toast};

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

fn dialog(window: &Rc<Window>, files: Vec<gio::File>, apps: Applications) {
    backend::dialog(window.window.upcast_ref(), Some(window.overlay.clone()), files, apps, false);
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
