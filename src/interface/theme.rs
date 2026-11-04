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
pub(super) const SECTIONS: &[(&str, &str)] = &[
    ("places", "blue"),
    ("pinned", "amber"),
    ("cloud", "teal"),
    ("devices", "green"),
];

thread_local! {
    static SECTIONS_PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
}

fn swatch_colour(name: &str) -> Option<String> {
    nyx_theme::swatches()
        .into_iter()
        .find(|(swatch, _)| swatch == name)
        .map(|(_, colour)| colour)
}

/// A section's colour without a pick in Settings: the theme's section accent,
/// else the section's default swatch.
pub(super) fn theme_section_colour(section: &str) -> String {
    let colour = nyx_theme::section_accent("files", section);
    SECTIONS
        .iter()
        .find(|(id, _)| *id == section)
        .and_then(|(_, name)| swatch_colour(name))
        .filter(|_| colour == nyx_theme::app_accent("files"))
        .unwrap_or(colour)
}

/// The colour a section's heading shows: the swatch picked in Settings, else
/// the theme's.
fn section_colour(section: &str) -> String {
    PreferenceManager::shared()
        .sidebar_section_color(section)
        .and_then(|name| swatch_colour(&name))
        .unwrap_or_else(|| theme_section_colour(section))
}

fn sections_css() -> String {
    let mut css = String::new();
    for (section, _) in SECTIONS {
        let colour = section_colour(section);
        css.push_str(&format!(
            "window.files-window .section-{section} {{ color: {colour}; }}\n\
             window.files-window .nav-group.section-{section} {{ border-bottom-color: alpha({colour}, 0.35); }}\n\
             window.files-window .sidebar.colored-icons button.nav-item.section-icon-{section} image {{ color: {colour}; }}\n"
        ));
    }
    css
}

/// Restyles the sidebar sections after a section colour changed.
pub(super) fn refresh_sections() {
    SECTIONS_PROVIDER.with(|provider| provider.load_from_string(&sections_css()));
}

fn install_sections() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    SECTIONS_PROVIDER.with(|provider| {
        gtk::style_context_add_provider_for_display(
            &display,
            provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 2,
        );
    });
    refresh_sections();
    nyx_theme::on_change(refresh_sections);
}
