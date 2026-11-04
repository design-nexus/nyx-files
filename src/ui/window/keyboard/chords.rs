// SPDX-License-Identifier: MIT

//! Armed 10xer chords. Once a chord's mark shows in the footer, the next key
//! completes or cancels that chord and reaches no other handler. Text-size
//! shortcuts, shortcut-reference keys, and Escape while autoscroll is running
//! are earlier capture handlers: they drop the mark first, then run, the same
//! way Ctrl+, opens Settings over a canceled chord.


use gtk::{
    gdk::ModifierType as Modifiers,
    glib::{self},
};
use crate::{
    model::Location,
    ui::{
        keys::{KeyPress, bindings::*},
        window::home_directory,
    },
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum GoTarget {
    FirstItem,
    /// The folder holding the search hit under the cursor.
    HitFolder,
    Prompt,
    Pin(bool),
    /// `validate` routes URI places through mount-aware validation.
    Place {
        location: Location,
        validate: bool,
    },
    Folder {
        location: Location,
        missing: String,
    },
    Missing(String),
}

/// Resolves the second key of **g**. `pins` are the visible PINNED rows in
/// sidebar display order.
pub(crate) fn go_target(press: &KeyPress, pins: &[Location]) -> Option<GoTarget> {
    let place = |location, validate| Some(GoTarget::Place { location, validate });
    if press.is(GO_FIRST) {
        Some(GoTarget::FirstItem)
    } else if press.is(GO_FOLLOW) {
        Some(GoTarget::HitFolder)
    } else if press.is(GO_PROMPT.ignoring(Modifiers::SHIFT_MASK)) {
        Some(GoTarget::Prompt)
    } else if press.is(GO_PIN) {
        Some(GoTarget::Pin(true))
    } else if press.is(GO_UNPIN) {
        Some(GoTarget::Pin(false))
    } else if press.is(GO_HOME) {
        place(Location::local(home_directory()), false)
    } else if press.is(GO_CONFIG) {
        config_folder()
    } else if press.is(GO_TRASH) {
        place(Location::uri("trash:///"), false)
    } else if press.is(GO_NETWORK) {
        place(Location::uri("network:///"), true)
    } else if press.is(GO_RECENT) {
        place(Location::uri("recent:///"), false)
    } else if press.is(GO_DOWNLOADS) {
        user_folder(glib::UserDirectory::Downloads, "No Downloads folder")
    } else if press.is(GO_DOCUMENTS) {
        user_folder(glib::UserDirectory::Documents, "No Documents folder")
    } else if press.is(GO_MUSIC) {
        user_folder(glib::UserDirectory::Music, "No Music folder")
    } else if press.is(GO_PICTURES) {
        user_folder(glib::UserDirectory::Pictures, "No Pictures folder")
    } else if press.is(GO_VIDEOS) {
        user_folder(glib::UserDirectory::Videos, "No Videos folder")
    } else {
        let number = usize::from(press.digit_in(PIN_NUMBERS)?);
        Some(match pins.get(number - 1) {
            Some(location) => GoTarget::Place {
                location: location.clone(),
                validate: true,
            },
            None => {
                GoTarget::Missing(rust_i18n::t!("No pin %{number}", number = number).into_owned())
            }
        })
    }
}

fn config_folder() -> Option<GoTarget> {
    Some(GoTarget::Folder {
        location: Location::local(home_directory().join(".config")),
        missing: crate::i18n::tr("No .config folder"),
    })
}

fn user_folder(directory: glib::UserDirectory, missing: &str) -> Option<GoTarget> {
    let missing = crate::i18n::tr(missing);
    Some(match glib::user_special_dir(directory) {
        Some(path) => GoTarget::Folder {
            location: Location::local(path),
            missing,
        },
        None => GoTarget::Missing(missing),
    })
}


