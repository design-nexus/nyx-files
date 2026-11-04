// SPDX-License-Identifier: MIT

use std::cell::Cell;

use crate::ui::preferences::PreferenceManager;

thread_local! {
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn install() {
    if INSTALLED.with(|i| i.replace(true)) {
        return;
    }
    let _colours = crate::ui::theme::ThemeManager::shared();
    nyx_theme::use_design_2();
    nyx_theme::install(nyx_theme::Style {
        window_class: "files-window",
        base_skip: &[],
        css: concat!(include_str!("parts.css"), include_str!("style.css")),
        no_glow_css: "",
        extra_colors: None,
        prefs: || {
            let suite = nyx_theme::read_suite().unwrap_or_default();
            nyx_theme::ThemePrefs {
                mode: suite.mode,
                theme: suite.theme,
                glow: false,
                reduce_motion: PreferenceManager::shared().reduce_motion(),
            }
        },
        set_theme: Some(|mode, theme| {
            nyx_theme::update_suite(|suite| {
                suite.mode = mode;
                suite.theme = theme;
            });
        }),
    });
    install_sections();
}

/// Each sidebar group's colour, as Settings colours its groups: a theme swatch
/// per section, following the theme as it changes.
const SECTIONS: &[(&str, &str)] = &[
    ("places", "blue"),
    ("pinned", "amber"),
    ("devices", "green"),
    ("cloud", "teal"),
];

fn sections_css() -> String {
    let swatches = nyx_theme::swatches();
    let mut css = String::new();
    for (section, swatch) in SECTIONS {
        let colour = nyx_theme::section_accent("files", section);
        let colour = swatches
            .iter()
            .find(|(name, _)| name == swatch)
            .map(|(_, c)| c.clone())
            .filter(|_| colour == nyx_theme::app_accent("files"))
            .unwrap_or(colour);
        css.push_str(&format!(
            "window.files-window .section-{section} {{ color: {colour}; }}\n\
             window.files-window .nav-group.section-{section} {{ border-bottom-color: alpha({colour}, 0.35); }}\n"
        ));
    }
    css
}

fn install_sections() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&sections_css());
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
    );
    nyx_theme::on_change(move || provider.load_from_string(&sections_css()));
}
