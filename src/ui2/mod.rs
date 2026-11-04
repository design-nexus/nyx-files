// SPDX-License-Identifier: MIT

//! The Nyx 2.0 interface, built on nyx-ui. Runs instead of `crate::ui` when
//! `NYX_FILES_UI=2` is set, as its own (non-unique) instance.

mod actions;
mod appearance;
mod chooser;
mod columns;
mod customize;
mod devices;
mod drives;
mod icons;
mod icons_theme;
mod list;
mod menus;
mod model;
mod mount;
mod open_with;
mod operations;
mod properties;
mod search;
mod settings;
mod sidebar;
mod tab;
mod tenxer;
mod theme;
mod window;

pub(crate) const ENV: &str = "NYX_FILES_UI";

pub(crate) fn enabled() -> bool {
    std::env::var(ENV).is_ok_and(|value| value == "2")
}

pub(crate) fn present(application: &gtk::Application) {
    theme::install();
    if let Ok(kind) = std::env::var("NYX_FILES_CHOOSER") {
        chooser::present_for_development(&kind);
        return;
    }
    window::Window::open(application, None);
}

/// The portal's chooser on the 2.0 interface.
pub(crate) static CHOOSER_UI: crate::portal::ChooserUi = crate::portal::ChooserUi {
    prepare: chooser::prepare,
    present: |request, cancelled, completion| chooser::present(request, cancelled, completion),
    cancel: chooser::cancel,
    flush: crate::ui::preferences::flush_pending_folder_views,
};

pub(crate) fn present_open(application: &gtk::Application, file: &gio::File) {
    use gio::prelude::FileExt;
    theme::install();
    let location = crate::model::Location::uri(file.uri().to_string());
    window::Window::open(application, Some(location));
}
