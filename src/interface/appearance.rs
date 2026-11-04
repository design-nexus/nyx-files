// SPDX-License-Identifier: MIT

//! The Appearance popover (grouping, Columns sizes, the preview panel, hidden
//! files, List columns, density, text size, icon size) and the stylesheet that
//! applies density and text size.

use std::{cell::RefCell, rc::Rc};

use gtk::{gdk, glib, prelude::*};

use crate::ui::{
    browser_modes::{BrowserDensity, BrowserMode, DensityScale, FrameDensity},
    preferences::{ListColumnId, PreferenceManager, TextSize},
};

use super::{tab::Tab, window::Window};

thread_local! {
    static PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
}

fn css() -> String {
    let prefs = PreferenceManager::shared();
    let scale = nyx_theme::read_suite().map_or(1.0, |s| s.text_scale);
    let text = f64::from(prefs.text_size().root_font_px()) * scale;
    let content = f64::from(prefs.browser_density().px());
    let frame = prefs.frame_density().px();
    let step = (content - f64::from(BrowserDensity::COMPACT_PX))
        / f64::from(BrowserDensity::AIRY_PX - BrowserDensity::COMPACT_PX);
    let between = |compact: f64, airy: f64| (compact + (airy - compact) * step).max(0.0).round();
    let row = between(30.0, 40.0);
    let tile = between(6.0, 12.0);
    let half = f64::from(frame) / 2.0;
    format!(
        "window.files-window {{ font-size: {text:.1}px; }}\n\
         window.files-window columnview.file-list > listview > row,\n\
         window.files-window listview.file-column > row {{ min-height: {row}px; }}\n\
         window.files-window gridview.file-grid > child {{ padding: {tile}px 2px; }}\n\
         window.files-window .app-header {{ padding-top: {frame}px; padding-bottom: {frame}px; }}\n\
         window.files-window .tab-row {{ padding-top: {half}px; padding-bottom: {half}px; }}\n\
         window.files-window .sidebar button.nav-item {{ margin-top: {half}px; margin-bottom: {half}px; }}\n\
         window.files-window .preview-header {{ padding-top: {frame}px; padding-bottom: {frame}px; }}\n"
    )
}

/// Applies density and text size now and whenever they change.
pub(super) fn install(anchor: &impl IsA<gtk::Widget>) {
    PROVIDER.with(|provider| {
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 5,
            );
        }
        provider.load_from_string(&css());
    });
    let reload = || PROVIDER.with(|p| p.load_from_string(&css()));
    let prefs = PreferenceManager::shared();
    prefs.bind_preference(anchor, PreferenceManager::text_size, move |_, _| reload());
    prefs.bind_preference(anchor, PreferenceManager::browser_density, move |_, _| {
        reload()
    });
    prefs.bind_preference(anchor, PreferenceManager::frame_density, move |_, _| {
        reload()
    });
    nyx_theme::on_change(reload);
}

pub(super) fn step_text(delta: i32) {
    let prefs = PreferenceManager::shared();
    let size = if delta == 0 {
        TextSize::DEFAULT
    } else {
        prefs.text_size().stepped(delta)
    };
    prefs.set_text_size(size);
}

/// A row that toggles; its check follows `read`.
fn check(
    title: &str,
    read: impl Fn() -> bool + 'static,
    toggle: impl Fn() + 'static,
) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("menu-item");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let mark = gtk::Image::from_icon_name("object-select-symbolic");
    mark.add_css_class("menu-icon");
    mark.set_size_request(16, -1);
    let label = gtk::Label::new(Some(&crate::i18n::tr(title)));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&mark);
    row.append(&label);
    button.set_child(Some(&row));
    let read = Rc::new(read);
    let sync = {
        let (mark, button, read) = (mark.clone(), button.clone(), read.clone());
        move || {
            let on = read();
            mark.set_opacity(if on { 1.0 } else { 0.0 });
            if on {
                button.add_css_class("checked");
            } else {
                button.remove_css_class("checked");
            }
        }
    };
    sync();
    button.connect_clicked(move |_| {
        toggle();
        sync();
    });
    button
}

/// The folder showing's sort, as the tab's browser has it.
fn sort_of(current: &CurrentTab) -> Option<(Rc<Tab>, usize, crate::model::ViewPreferences)> {
    let tab = current()?;
    let depth = tab.browser.active_depth()?;
    let preferences = tab.browser.column_preferences(depth)?;
    Some((tab, depth, preferences))
}

fn action(title: &str, run: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::with_label(&crate::i18n::tr(title));
    button.add_css_class("flat");
    button.add_css_class("menu-item");
    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
        label.set_xalign(0.0);
        label.set_margin_start(26);
    }
    button.connect_clicked(move |_| run());
    button
}

fn append_sort(body: &gtk::Box, current: &CurrentTab) {
    use crate::model::{SortDirection, SortKey};
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&heading("Sort"));
    let checks: Rc<RefCell<Vec<gtk::Button>>> = Rc::default();
    for (key, title) in [
        (SortKey::Name, "Name"),
        (SortKey::Modified, "Modified"),
        (SortKey::Size, "Size"),
        (SortKey::Type, "Kind"),
    ] {
        let (c1, c2, all) = (current.clone(), current.clone(), checks.clone());
        let button = check(
            title,
            move || sort_of(&c1).is_some_and(|(_, _, p)| p.sort_key == key),
            move || {
                if let Some((tab, depth, p)) = sort_of(&c2) {
                    tab.browser.set_sort(depth, key, p.sort_direction);
                }
                if let Some(popover) = all
                    .borrow()
                    .first()
                    .and_then(|b| b.ancestor(gtk::Popover::static_type()))
                    .and_downcast::<gtk::Popover>()
                {
                    popover.popdown();
                }
            },
        );
        checks.borrow_mut().push(button.clone());
        body.append(&button);
    }
    let (c1, c2) = (current.clone(), current.clone());
    body.append(&check(
        "Descending",
        move || sort_of(&c1).is_some_and(|(_, _, p)| p.sort_direction == SortDirection::Descending),
        move || {
            if let Some((tab, depth, p)) = sort_of(&c2) {
                let direction = match p.sort_direction {
                    SortDirection::Ascending => SortDirection::Descending,
                    SortDirection::Descending => SortDirection::Ascending,
                };
                tab.browser.set_sort(depth, p.sort_key, direction);
            }
        },
    ));
    let (c1, c2) = (current.clone(), current.clone());
    body.append(&check(
        "Folders first",
        move || sort_of(&c1).is_some_and(|(_, _, p)| p.folders_first),
        move || {
            if let Some((tab, depth, p)) = sort_of(&c2) {
                tab.browser.set_folders_first(depth, !p.folders_first);
            }
        },
    ));
    let c = current.clone();
    body.append(&action("Make this the default sort", move || {
        if let Some((_, _, p)) = sort_of(&c) {
            PreferenceManager::shared().set_default_sort(p.sort_key, p.sort_direction);
        }
    }));
    let c = current.clone();
    body.append(&action("Reset to default sort", move || {
        if let Some(location) = c().and_then(|t| t.location()) {
            PreferenceManager::shared().reset_folder_sort(&location);
        }
    }));
}

fn heading(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&crate::i18n::tr(text).to_uppercase()));
    label.set_xalign(0.0);
    label.add_css_class("menu-heading");
    label
}

/// A slider with Compact and Airy marks that snap.
fn density<T: DensityScale>(current: T, set: impl Fn(T) + 'static) -> gtk::Scale {
    let scale =
        gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, f64::from(T::MAX_PX), 1.0);
    scale.set_value(f64::from(current.px()));
    scale.set_draw_value(false);
    scale.add_mark(
        f64::from(T::COMPACT_PX),
        gtk::PositionType::Bottom,
        Some(&crate::i18n::tr("Compact")),
    );
    scale.add_mark(
        f64::from(T::AIRY_PX),
        gtk::PositionType::Bottom,
        Some(&crate::i18n::tr("Airy")),
    );
    scale.set_hexpand(true);
    scale
        .connect_value_changed(move |s| set(T::from_px(s.value().round().clamp(0.0, 255.0) as u8)));
    scale
}

fn labelled(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("appearance-row");
    let label = gtk::Label::new(Some(&crate::i18n::tr(title)));
    label.set_xalign(0.0);
    label.set_width_chars(8);
    row.append(&label);
    row.append(control);
    row
}

/// The tab the menu acts on: the window's current one, or the chooser's.
pub(super) type CurrentTab = Rc<dyn Fn() -> Option<Rc<Tab>>>;

fn append_columns(body: &gtk::Box) {
    body.append(&heading("Columns"));
    for id in [
        ListColumnId::Mode,
        ListColumnId::Size,
        ListColumnId::Type,
        ListColumnId::Modified,
    ] {
        let title = match id {
            ListColumnId::Type => "Kind",
            other => other.title(),
        };
        body.append(&check(
            title,
            move || {
                !PreferenceManager::shared()
                    .list_hidden_columns()
                    .contains(&id)
            },
            move || {
                let p = PreferenceManager::shared();
                let mut hidden = p.list_hidden_columns();
                if let Some(i) = hidden.iter().position(|h| *h == id) {
                    hidden.remove(i);
                } else {
                    hidden.push(id);
                }
                p.set_list_hidden_columns(hidden);
            },
        ));
    }
}

fn append_icon_size(body: &gtk::Box, current: &CurrentTab) {
    body.append(&heading("Thumbnail size"));
    let size = current().map_or(96, |t| t.icons_size());
    let value = gtk::Label::new(Some(&format!("{size} px")));
    value.add_css_class("num");
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 32.0, 256.0, 8.0);
    scale.set_value(f64::from(size));
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_size_request(200, -1);
    let c = current.clone();
    let v = value.clone();
    scale.connect_value_changed(move |s| {
        let px = s.value().round() as i32;
        v.set_text(&format!("{px} px"));
        if let Some(t) = c() {
            t.set_icons_size(px);
        }
    });
    let row = labelled("Size", &scale);
    row.append(&value);
    body.append(&row);
    let c = current.clone();
    body.append(&action("Reset to default size", move || {
        if let Some(t) = c() {
            t.resize_icons(0);
        }
    }));
}

fn menu_body() -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 1);
    body.add_css_class("menu-list");
    body.add_css_class("appearance-menu");
    body
}

/// The sort menu of the header's sort button.
pub(super) fn sort_menu(current: &CurrentTab) -> gtk::Box {
    let body = menu_body();
    append_sort(&body, current);
    if let Some(first) = body.first_child() {
        body.remove(&first);
    }
    body
}

/// The thumbnail size of the header's Icons button.
pub(super) fn thumbnail_menu(current: &CurrentTab) -> gtk::Box {
    let body = menu_body();
    append_icon_size(&body, current);
    body
}

fn content(current: &CurrentTab) -> gtk::Box {
    let prefs = PreferenceManager::shared();
    let mode = current().map_or_else(|| prefs.browser_mode(), |t| t.mode());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 1);
    body.add_css_class("menu-list");
    body.add_css_class("appearance-menu");
    body.append(&heading("View"));
    if mode == BrowserMode::List {
        body.append(&check(
            "Group by file type",
            || PreferenceManager::shared().group_by_type(),
            || {
                let p = PreferenceManager::shared();
                p.set_group_by_type(!p.group_by_type());
            },
        ));
    }
    if mode == BrowserMode::Columns {
        body.append(&check(
            "Show sizes in Columns view",
            || PreferenceManager::shared().columns_show_sizes(),
            || {
                let p = PreferenceManager::shared();
                p.set_columns_show_sizes(!p.columns_show_sizes());
            },
        ));
    }
    {
        let (c1, c2) = (current.clone(), current.clone());
        body.append(&check(
            "Preview panel",
            move || c1().is_some_and(|t| t.preview.is_open()),
            move || {
                if let Some(t) = c2() {
                    t.toggle_preview();
                }
            },
        ));
    }
    {
        let (c1, c2) = (current.clone(), current.clone());
        body.append(&check(
            "Hidden files",
            move || c1().is_some_and(|t| t.shows_hidden()),
            move || {
                if let Some(t) = c2() {
                    t.browser.toggle_hidden();
                }
            },
        ));
    }
    append_sort(&body, current);
    if mode == BrowserMode::List {
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        append_columns(&body);
    }
    if mode == BrowserMode::Icons {
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        append_icon_size(&body, current);
    }
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&heading("Density"));
    body.append(&labelled(
        "Frame",
        &density(prefs.frame_density(), |d: FrameDensity| {
            PreferenceManager::shared().set_frame_density(d)
        }),
    ));
    body.append(&labelled(
        "Content",
        &density(prefs.browser_density(), |d: BrowserDensity| {
            PreferenceManager::shared().set_browser_density(d)
        }),
    ));
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&heading("Text size"));
    let text = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    text.add_css_class("appearance-row");
    let smaller = nyx_ui::widgets::icon_button("zoom-out-symbolic", &crate::i18n::tr("Smaller"));
    let value = gtk::Label::new(None);
    value.set_hexpand(true);
    value.add_css_class("num");
    let larger = nyx_ui::widgets::icon_button("zoom-in-symbolic", &crate::i18n::tr("Larger"));
    let reset = nyx_ui::widgets::icon_button("zoom-original-symbolic", &crate::i18n::tr("Reset"));
    for b in [&smaller, &larger, &reset] {
        b.remove_css_class("flat");
    }
    text.append(&smaller);
    text.append(&value);
    text.append(&larger);
    text.append(&reset);
    let show = {
        let value = value.clone();
        move || {
            value.set_text(&format!(
                "{} px",
                PreferenceManager::shared().text_size().root_font_px()
            ))
        }
    };
    show();
    let s = show.clone();
    smaller.connect_clicked(move |_| {
        step_text(-1);
        s();
    });
    let s = show.clone();
    larger.connect_clicked(move |_| {
        step_text(1);
        s();
    });
    reset.connect_clicked(move |_| {
        step_text(0);
        show();
    });
    body.append(&text);
    body
}

/// The header button; the popover is built fresh each time it opens.
pub(super) fn button(window: &Rc<Window>) -> gtk::MenuButton {
    let weak = Rc::downgrade(window);
    button_for(Rc::new(move || weak.upgrade().and_then(|w| w.current())))
}

/// The header button acting on whichever tab `current` gives.
pub(super) fn button_for(current: CurrentTab) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_icon_name("preferences-desktop-appearance-symbolic");
    button.add_css_class("bar-button");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(&crate::i18n::tr("Appearance")));
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.add_css_class("menu-popover");
    button.set_popover(Some(&popover));
    popover.connect_show(move |p| p.set_child(Some(&content(&current))));
    let _ = glib::Propagation::Proceed;
    button
}
