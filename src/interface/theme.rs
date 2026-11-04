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
    watch_icon_theme();
}

/// Waits for a burst of icon theme writes to end before redrawing.
const ICON_THEME_SETTLE: std::time::Duration = std::time::Duration::from_millis(800);

/// Redraws file and folder icons when the icon theme changes: a different
/// theme name, or the same theme regenerated in place for a new color theme.
fn watch_icon_theme() {
    use gtk::{gio, glib, prelude::*};
    use std::{cell::RefCell, rc::Rc};

    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let redraw = Rc::new(move || {
        if let Some(previous) = timer.borrow_mut().take() {
            previous.remove();
        }
        let done = Rc::downgrade(&timer);
        *timer.borrow_mut() = Some(glib::timeout_add_local_once(ICON_THEME_SETTLE, move || {
            if let Some(timer) = done.upgrade() {
                timer.borrow_mut().take();
            }
            crate::ui::thumbnail::refresh_all_customized_icons();
        }));
    });
    let monitors: Rc<RefCell<Vec<gio::FileMonitor>>> = Rc::default();
    let watch = {
        let (monitors, redraw) = (monitors.clone(), redraw.clone());
        move |settings: &gtk::Settings| {
            let Some(name) = settings.gtk_icon_theme_name() else {
                monitors.borrow_mut().clear();
                return;
            };
            let roots = std::iter::once(glib::user_data_dir())
                .chain(glib::system_data_dirs())
                .chain(std::iter::once(glib::home_dir().join(".icons")));
            let watched = roots
                .map(|root| root.join("icons").join(name.as_str()))
                .filter(|dir| dir.is_dir())
                .filter_map(|dir| {
                    let monitor = gio::File::for_path(dir)
                        .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
                        .ok()?;
                    let redraw = redraw.clone();
                    monitor.connect_changed(move |_, _, _, _| redraw());
                    Some(monitor)
                })
                .collect();
            *monitors.borrow_mut() = watched;
        }
    };
    watch(&settings);
    settings.connect_gtk_icon_theme_name_notify(move |settings| {
        watch(settings);
        redraw();
    });
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
