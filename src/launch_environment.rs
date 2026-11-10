// SPDX-License-Identifier: MIT

//! Variables Files sets in its own environment for GTK when it restarts
//! itself. Programs Files starts get the user's values back instead.

use std::{
    ffi::{OsStr, OsString},
    process::Command,
    sync::OnceLock,
};

use gtk::{gio, prelude::*};

/// Present only when Files added `no-portals` to `GDK_DEBUG`; holds the value
/// before, empty when it was unset.
pub(crate) const GDK_DEBUG_BEFORE: &str = "NYX_FILES_GDK_DEBUG_BEFORE";

/// Each variable with the value programs Files starts should see (`None` to unset).
fn restored() -> &'static [(&'static str, Option<OsString>)] {
    static RESTORED: OnceLock<Vec<(&'static str, Option<OsString>)>> = OnceLock::new();
    RESTORED.get_or_init(|| {
        restored_from(
            std::env::var_os(GDK_DEBUG_BEFORE).as_deref(),
            std::env::var_os(crate::CAIRO_SELECTED_BY_STRATA).as_deref(),
            std::env::var_os("GSK_RENDERER").as_deref(),
        )
    })
}

fn restored_from(
    gdk_debug_before: Option<&OsStr>,
    cairo_selected: Option<&OsStr>,
    renderer: Option<&OsStr>,
) -> Vec<(&'static str, Option<OsString>)> {
    let mut restored = Vec::new();
    if let Some(before) = gdk_debug_before {
        restored.push(("GDK_DEBUG", (!before.is_empty()).then(|| before.to_owned())));
        restored.push((GDK_DEBUG_BEFORE, None));
    }
    if cairo_selected == Some(OsStr::new("1")) && renderer == Some(OsStr::new("cairo")) {
        restored.push(("GSK_RENDERER", None));
        restored.push((crate::CAIRO_SELECTED_BY_STRATA, None));
    }
    restored
}

pub(crate) fn restore(command: &mut Command) -> &mut Command {
    for (name, value) in restored() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

pub(crate) fn restore_in(context: &impl IsA<gio::AppLaunchContext>) {
    for (name, value) in restored() {
        match value {
            Some(value) => context.setenv(name, value),
            None => context.unsetenv(name),
        }
    }
}

#[cfg(test)]
mod tests;
