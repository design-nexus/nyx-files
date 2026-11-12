// SPDX-License-Identifier: MIT

//! With its portals off, GTK reads none of the desktop's settings and draws
//! with its defaults: Adwaita icons in place of the chosen icon theme, no
//! cursor theme, unhinted text. Files reads them from GSettings itself then,
//! as GTK does through the settings portal, and follows them as they change.

use std::cell::RefCell;

use gtk::{gio, prelude::*};

const INTERFACE: &str = "org.gnome.desktop.interface";
const WM_PREFERENCES: &str = "org.gnome.desktop.wm.preferences";

const INTERFACE_KEYS: &[&str] = &[
    "icon-theme",
    "gtk-theme",
    "cursor-theme",
    "cursor-size",
    "font-antialiasing",
    "text-scaling-factor",
];
const WM_PREFERENCES_KEYS: &[&str] = &["button-layout"];

thread_local! {
    static FOLLOWED: RefCell<Vec<gio::Settings>> = const { RefCell::new(Vec::new()) };
}

/// Whether `gdk_debug` turns GTK's portals off.
pub(crate) fn turns_portals_off(gdk_debug: &std::ffi::OsStr) -> bool {
    gdk_debug
        .to_string_lossy()
        .split([',', ':', ' '])
        .any(|flag| flag == crate::GDK_NO_PORTALS)
}

/// Gives GTK the desktop's settings when its portals are off. Needs GTK started.
pub(crate) fn follow_when_portals_are_off() {
    if !std::env::var_os(crate::GDK_DEBUG).is_some_and(|value| turns_portals_off(&value)) {
        return;
    }
    let Some(target) = gtk::Settings::default() else {
        return;
    };
    for (schema, keys) in [(INTERFACE, INTERFACE_KEYS), (WM_PREFERENCES, WM_PREFERENCES_KEYS)] {
        let Some(source) = desktop_schema(schema) else {
            continue;
        };
        for key in keys {
            apply(&target, &source, key);
        }
        let target = target.clone();
        source.connect_changed(None, move |source, key| apply(&target, source, key));
        FOLLOWED.with(|followed| followed.borrow_mut().push(source));
    }
}

/// `None` where the schema isn't installed; GSettings aborts on a missing one.
fn desktop_schema(id: &str) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(id, true)?;
    Some(gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None))
}

fn apply(target: &gtk::Settings, source: &gio::Settings, key: &str) {
    let has = |key: &str| source.settings_schema().is_some_and(|schema| schema.has_key(key));
    if !has(key) {
        return;
    }
    let name = || Some(source.string(key)).filter(|name| !name.is_empty());
    match key {
        "icon-theme" => {
            if let Some(name) = name() {
                target.set_gtk_icon_theme_name(Some(&name));
            }
        }
        "gtk-theme" => {
            if let Some(name) = name() {
                target.set_gtk_theme_name(Some(&name));
            }
        }
        "cursor-theme" => {
            if let Some(name) = name() {
                target.set_gtk_cursor_theme_name(Some(&name));
            }
        }
        "cursor-size" => target.set_gtk_cursor_theme_size(source.int(key)),
        "font-antialiasing" | "font-hinting" | "font-rgba-order"
            if has("font-hinting") && has("font-rgba-order") =>
        {
            let font = font_rendering(
                &source.string("font-antialiasing"),
                &source.string("font-hinting"),
                &source.string("font-rgba-order"),
            );
            target.set_gtk_xft_antialias(font.antialias);
            target.set_gtk_xft_hinting(font.hinting);
            target.set_gtk_xft_hintstyle(Some(font.hint_style));
            target.set_gtk_xft_rgba(Some(font.rgba));
        }
        "text-scaling-factor" => target.set_gtk_xft_dpi(xft_dpi(source.double(key))),
        "button-layout" => target.set_gtk_decoration_layout(Some(&source.string(key))),
        _ => {}
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FontRendering {
    antialias: i32,
    hinting: i32,
    hint_style: &'static str,
    rgba: &'static str,
}

/// GTK's Xft values for the desktop's font settings, as GDK translates them.
fn font_rendering(antialiasing: &str, hinting: &str, rgba_order: &str) -> FontRendering {
    let (hinting, hint_style) = match hinting {
        "none" => (0, "hintnone"),
        "medium" => (1, "hintmedium"),
        "full" => (1, "hintfull"),
        _ => (1, "hintslight"),
    };
    let rgba = match (antialiasing, rgba_order) {
        ("rgba", "bgr") => "bgr",
        ("rgba", "vrgb") => "vrgb",
        ("rgba", "vbgr") => "vbgr",
        ("rgba", _) => "rgb",
        _ => "none",
    };
    FontRendering {
        antialias: i32::from(antialiasing != "none"),
        hinting,
        hint_style,
        rgba,
    }
}

/// GTK's Xft DPI (1024ths of a dot per inch) for a text scaling factor.
fn xft_dpi(text_scaling_factor: f64) -> i32 {
    (96.0 * text_scaling_factor * 1024.0).round() as i32
}

#[cfg(test)]
mod tests;
