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
    crate::ui::theme::tokens_only();
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
}
