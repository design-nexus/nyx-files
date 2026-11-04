// SPDX-License-Identifier: MIT

//! The Nyx 2.0 interface, built on nyx-ui. Runs instead of `crate::ui` when
//! `NYX_FILES_UI=2` is set, as its own (non-unique) instance.

mod actions;
mod columns;
mod icons;
mod list;
mod model;
mod places;
mod settings;
mod tab;
mod theme;
mod window;

pub(crate) const ENV: &str = "NYX_FILES_UI";

pub(crate) fn enabled() -> bool {
    std::env::var(ENV).is_ok_and(|value| value == "2")
}

pub(crate) fn present(application: &gtk::Application) {
    theme::install();
    window::Window::open(application, None);
}

pub(crate) fn present_open(application: &gtk::Application, file: &gio::File) {
    use gio::prelude::FileExt;
    theme::install();
    let location = crate::model::Location::uri(file.uri().to_string());
    window::Window::open(application, Some(location));
}
