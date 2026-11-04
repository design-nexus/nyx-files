// SPDX-License-Identifier: MIT

//! Files' interface, built on nyx-ui.

mod actions;
mod appearance;
mod chooser;
mod columns;
mod customize;
mod devices;
mod drives;
mod icons;
mod icons_theme;
mod launch;
mod list;
mod menus;
mod model;
mod mount;
mod open_with;
mod operations;
mod preview_keys;
mod properties;
mod search;
mod settings;
mod sidebar;
mod tab;
mod tenxer;
mod theme;
mod window;

pub(crate) fn present(application: &gtk::Application) {
    theme::install();
    if let Ok(kind) = std::env::var("NYX_FILES_CHOOSER") {
        chooser::present_for_development(&kind);
        return;
    }
    launch::present(application);
}

pub(crate) fn present_open(application: &gtk::Application, file: &gio::File) {
    theme::install();
    launch::present_open(application, file);
}

pub(crate) fn present_reveal(
    application: &gtk::Application,
    request: crate::adapters::RevealRequest,
) {
    theme::install();
    launch::present_reveal(application, request);
}

pub(crate) fn present_unlock(application: &gtk::Application, target: crate::ui::UnlockTarget) {
    theme::install();
    launch::present_unlock(application, target);
}

/// The portal's chooser on the 2.0 interface.
pub(crate) static CHOOSER_UI: crate::portal::ChooserUi = crate::portal::ChooserUi {
    prepare: chooser::prepare,
    present: |request, cancelled, completion| chooser::present(request, cancelled, completion),
    cancel: chooser::cancel,
    flush: crate::ui::preferences::flush_pending_folder_views,
};
