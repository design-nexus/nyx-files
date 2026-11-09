// SPDX-License-Identifier: MIT

//! Settings as the Nyx settings card: Files' groups, then Appearance, Keyboard
//! and About, which also take Files' own display, keyboard and diagnostics rows.

use std::{cell::RefCell, rc::Rc};

use gtk::{gio, prelude::*};
use nyx_ui::{
    settings_card::{self, About, Spec, Standard},
    widgets,
};

use crate::{
    model::{SortDirection, SortKey},
    services::CrossVolumeDropStrategy,
    ui::{
        browser_modes::{BrowserMode, ClickCount},
        preferences::{InterfaceRenderer, PreferenceManager, PreviewStyle},
        settings::translated,
    },
};

use super::window::Window;

mod cloud;

const GROUPS: &[(&str, &str, &str)] = &[
    (
        "General",
        "preferences-system-symbolic",
        "Start folder, tabs, language, dates",
    ),
    (
        "Browsing",
        "folder-symbolic",
        "Opening items, sorting, columns",
    ),
    (
        "Sidebar",
        "sidebar-show-symbolic",
        "Places shown, cloud storage",
    ),
    (
        "Search",
        "system-search-symbolic",
        "Typing to search, limits, exclusions",
    ),
    (
        "Previews",
        "view-reveal-symbolic",
        "Preview style, media, thumbnails",
    ),
    (
        "Transfers",
        "folder-download-symbolic",
        "Dropping files, network folders",
    ),
    (
        "Actions",
        "system-run-symbolic",
        "Your own commands for files",
    ),
    (
        "Integrations",
        "emblem-system-symbolic",
        "System file manager, encrypted volumes",
    ),
];

const SPEC_DESCRIPTION: &str = "A fast, keyboard-first file manager.";

const SPEC: Spec = Spec {
    groups: GROUPS,
    search_text: None,
    standard: Some(Standard {
        app: "files",
        about: About {
            name: "Files",
            icon: crate::APPLICATION_ID,
            version: env!("CARGO_PKG_VERSION"),
            description: SPEC_DESCRIPTION,
            licence: "GPL-3.0-or-later",
            website: "https://github.com/design-nexus/nyx-files",
            issues: "https://github.com/design-nexus/nyx-files/issues",
            credits: &[],
        },
        shortcuts: &[],
        reduce_motion: || PreferenceManager::shared().reduce_motion(),
        set_reduce_motion: |on| PreferenceManager::shared().set_reduce_motion(on),
    }),
};

thread_local! {
    static BODY: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
}

pub(super) fn open(window: &Window) {
    let fresh = BODY.with(|b| b.borrow().is_none());
    settings_card::open(&window.overlay, &window.window, SPEC, || {
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("settings-page");
        body.add_css_class("files-settings");
        build(&body);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&body)
            .vexpand(true)
            .build();
        BODY.with(|b| *b.borrow_mut() = Some(body.clone()));
        (root.upcast(), body)
    });
    if fresh && let Some(body) = BODY.with(|b| b.borrow().clone()) {
        extend_standard(&body);
    }
}

type Read = fn(&PreferenceManager) -> bool;
type Write = fn(&PreferenceManager, bool);

/// A group of the card: its rows go in sections, each a titled list.
struct Group {
    wrapper: gtk::Box,
}

impl Group {
    fn new(body: &gtk::Box, name: &str) -> Group {
        let wrapper = gtk::Box::new(gtk::Orientation::Vertical, 0);
        wrapper.add_css_class("settings-group");
        wrapper.set_widget_name(name);
        let title = gtk::Label::new(Some(&crate::i18n::tr(name).to_uppercase()));
        title.add_css_class("group-title");
        title.set_xalign(0.0);
        wrapper.append(&title);
        body.append(&wrapper);
        Group { wrapper }
    }

    fn section(&self, title: &str) -> gtk::Box {
        card(&self.wrapper, (!title.is_empty()).then(|| section_title(title)))
    }
}

/// A card of rows appended to `container`, under `heading` when there is one.
fn card(container: &gtk::Box, heading: Option<gtk::Label>) -> gtk::Box {
    if let Some(heading) = heading {
        container.append(&heading);
    }
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("group-list");
    container.append(&list);
    list
}

/// A section heading for user content, such as an account name, which is not translated.
fn untranslated_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&title.to_uppercase()));
    label.add_css_class("menu-heading");
    label.add_css_class("files-section");
    label.set_xalign(0.0);
    label
}

/// A row of only a description, for notes and empty states.
fn note_row(text: &str) -> gtk::Box {
    let r = row("", text, None);
    if let Some(title) = r.first_child().and_then(|text| text.first_child()) {
        title.set_visible(false);
    }
    r
}

/// A row whose title and description are shown as given, such as machine text.
fn plain_row(title: &str, description: &str, control: Option<&gtk::Widget>) -> gtk::Box {
    widgets::row(title, &glib_escape(description), control)
}

fn toolkit_version() -> String {
    format!(
        "GTK {}.{}.{}",
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version()
    )
}

fn version_info() -> String {
    rust_i18n::t!(
        "Files %{value1}\n%{value2}\nCommit: %{value3}\nToolkit: %{value4}\nAuthor: %{value5}\nLicense: GPL-3.0-or-later",
        value1 = crate::build_info::displayed_version(),
        value2 = crate::i18n::tr(SPEC_DESCRIPTION),
        value3 = crate::build_info::COMMIT,
        value4 = toolkit_version(),
        value5 = crate::build_info::AUTHOR
    )
    .into_owned()
}

/// A small uppercase heading above a card inside a group.
fn section_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&crate::i18n::tr(title).to_uppercase()));
    label.add_css_class("menu-heading");
    label.add_css_class("files-section");
    label.set_xalign(0.0);
    label
}

fn row(title: &str, description: &str, control: Option<&gtk::Widget>) -> gtk::Box {
    widgets::row(
        &translated(title),
        &glib_escape(&translated(description)),
        control,
    )
}

fn glib_escape(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

fn switch(list: &gtk::Box, title: &str, description: &str, read: Read, write: Write) -> gtk::Box {
    let prefs = PreferenceManager::shared();
    let toggle = gtk::Switch::new();
    toggle.set_active(read(&prefs));
    toggle.update_property(&[gtk::accessible::Property::Label(&translated(title))]);
    toggle.connect_active_notify(move |t| write(&PreferenceManager::shared(), t.is_active()));
    prefs.bind_preference(&toggle, read, |widget, on| {
        if let Some(t) = widget.downcast_ref::<gtk::Switch>()
            && t.is_active() != on
        {
            t.set_active(on);
        }
    });
    let r = row(title, description, Some(toggle.upcast_ref()));
    list.append(&r);
    r
}

/// A dropdown of `choices` (label, value), read and written through the preferences.
fn choice<T: Copy + PartialEq + 'static>(
    list: &gtk::Box,
    title: &str,
    description: &str,
    choices: &[(&str, T)],
    read: fn(&PreferenceManager) -> T,
    write: fn(&PreferenceManager, T),
) {
    let prefs = PreferenceManager::shared();
    let labels: Vec<String> = choices
        .iter()
        .map(|(label, _)| crate::i18n::tr(label))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::from_strings(&labels);
    let values: Rc<Vec<T>> = Rc::new(choices.iter().map(|(_, v)| *v).collect());
    let current = read(&prefs);
    if let Some(i) = values.iter().position(|v| *v == current) {
        dropdown.set_selected(i as u32);
    }
    dropdown.update_property(&[gtk::accessible::Property::Label(&translated(title))]);
    let v = values.clone();
    dropdown.connect_selected_notify(move |d| {
        if let Some(value) = v.get(d.selected() as usize) {
            write(&PreferenceManager::shared(), *value);
        }
    });
    list.append(&row(title, description, Some(dropdown.upcast_ref())));
}

/// Joined buttons for a few choices.
fn segmented<T: Copy + PartialEq + 'static>(
    choices: &[(&str, T)],
    current: T,
    write: impl Fn(T) + 'static,
) -> gtk::Box {
    let options: Vec<(String, String)> = choices
        .iter()
        .enumerate()
        .map(|(i, (label, _))| (i.to_string(), crate::i18n::tr(label)))
        .collect();
    let current = choices
        .iter()
        .position(|(_, v)| *v == current)
        .unwrap_or(0)
        .to_string();
    let values: Vec<T> = choices.iter().map(|(_, v)| *v).collect();
    widgets::segmented(&options, &current, move |id| {
        if let Some(value) = id.parse::<usize>().ok().and_then(|i| values.get(i)) {
            write(*value);
        }
    })
}

fn build(body: &gtk::Box) {
    general(&Group::new(body, "General"));
    browsing(&Group::new(body, "Browsing"));
    sidebar(&Group::new(body, "Sidebar"));
    search(&Group::new(body, "Search"));
    previews(&Group::new(body, "Previews"));
    transfers(&Group::new(body, "Transfers"));
    let actions = Group::new(body, "Actions");
    crate::ui::settings::actions::append_actions(&actions.wrapper);
    let integrations = Group::new(body, "Integrations").section("");
    integrations.append(&crate::ui::portal_preferences::settings_row());
    integrations.append(&crate::ui::udiskie_preferences::settings_row());
}

fn general(group: &Group) {
    let list = group.section("");
    default_directory(&list);
    switch(
        &list,
        "Restore open tabs",
        "Reopen your tabs from the last session when launching Files without a target.",
        PreferenceManager::restore_tabs,
        PreferenceManager::set_restore_tabs,
    );
    choice(
        &list,
        "Language",
        "Auto-detect follows your system language. Restart Files to apply changes.",
        &crate::i18n::Language::CHOICES,
        PreferenceManager::language,
        PreferenceManager::set_language,
    );
    restart_row(
        &list,
        "Restart to apply the language",
        PreferenceManager::language_restart_required,
    );
    date_format(&list);
}

fn restart_row(list: &gtk::Box, title: &str, required: fn(&PreferenceManager) -> bool) {
    let button = gtk::Button::with_label(&crate::i18n::tr("Restart now"));
    button.connect_clicked(|_| {
        let application = gio::Application::default().and_downcast::<gtk::Application>();
        crate::ui::settings::restart(application.as_ref());
    });
    let r = row(title, "", Some(button.upcast_ref()));
    PreferenceManager::shared().bind_preference(&r, required, |widget, on| widget.set_visible(on));
    list.append(&r);
}

fn default_directory(list: &gtk::Box) {
    let prefs = PreferenceManager::shared();
    let text = |path: Option<std::path::PathBuf>| match path {
        Some(path) => crate::ui::settings::general::abbreviate_home(&path),
        None => crate::i18n::tr("Home"),
    };
    let choose = gtk::Button::with_label(&text(prefs.default_directory()));
    choose.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
        "Default directory",
    ))]);
    let reset = widgets::icon_button(
        "edit-undo-symbolic",
        &crate::i18n::tr("Restore the home directory as default"),
    );
    reset.set_sensitive(prefs.default_directory().is_some());
    reset.connect_clicked(|_| PreferenceManager::shared().set_default_directory(None));
    prefs.bind_preference(
        &choose,
        PreferenceManager::default_directory,
        move |widget, value| {
            if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                button.set_label(&text(value));
            }
        },
    );
    prefs.bind_preference(
        &reset,
        PreferenceManager::default_directory,
        |widget, value| {
            widget.set_sensitive(value.is_some());
        },
    );
    choose.connect_clicked(|button| {
        let window = button.root().and_downcast::<gtk::Window>();
        let dialog = gtk::FileDialog::builder()
            .title(crate::i18n::tr("Select default directory"))
            .modal(true)
            .build();
        dialog.select_folder(window.as_ref(), gio::Cancellable::NONE, |result| {
            if let Some(path) = result.ok().and_then(|file| file.path()) {
                PreferenceManager::shared().set_default_directory(Some(path));
            }
        });
    });
    let controls = widgets::hbox(6);
    controls.append(&choose);
    controls.append(&reset);
    list.append(&row(
        "Default directory",
        "Open this folder when launching Files without a target.",
        Some(controls.upcast_ref()),
    ));
}

fn date_format(list: &gtk::Box) {
    use crate::util::DateFormat;
    let choices = [DateFormat::Relative, DateFormat::Iso8601, DateFormat::Long];
    let labels: Vec<String> = choices
        .iter()
        .map(|f| {
            let name = match f {
                DateFormat::Relative => "Relative",
                DateFormat::Iso8601 => "ISO 8601",
                DateFormat::Long => "Long",
            };
            format!(
                "{} · {}",
                crate::i18n::tr(name),
                crate::util::modified_date_example(*f)
            )
        })
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let dropdown = gtk::DropDown::from_strings(&labels);
    dropdown.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
        "Modified date format",
    ))]);
    let current = PreferenceManager::shared().date_format();
    dropdown.set_selected(choices.iter().position(|f| *f == current).unwrap_or(0) as u32);
    dropdown.connect_selected_notify(move |d| {
        if let Some(format) = choices.get(d.selected() as usize) {
            PreferenceManager::shared().set_date_format(*format);
        }
    });
    list.append(&row(
        "Modified date format",
        "How file modified times appear in lists and details.",
        Some(dropdown.upcast_ref()),
    ));
}

fn browsing(group: &Group) {
    let opening = group.section("Opening items");
    for (label, mode) in [
        ("Columns view", BrowserMode::Columns),
        ("Icons view", BrowserMode::Icons),
        ("List view", BrowserMode::List),
    ] {
        let activation = PreferenceManager::shared().click_activation(mode);
        let controls = widgets::hbox(10);
        for (kind, files, current) in [
            ("Files", true, activation.files),
            ("Folders", false, activation.folders),
        ] {
            controls.append(&widgets::label(
                &crate::i18n::tr(kind),
                "settings-option-description",
            ));
            controls.append(&segmented(
                &[("Single", ClickCount::One), ("Double", ClickCount::Two)],
                current,
                move |count| {
                    let prefs = PreferenceManager::shared();
                    let mut activation = prefs.click_activation(mode);
                    if files {
                        activation.files = count;
                    } else {
                        activation.folders = count;
                    }
                    prefs.set_click_activation(mode, activation);
                },
            ));
        }
        opening.append(&row(label, "", Some(controls.upcast_ref())));
    }

    let list = group.section("Folders");
    switch(
        &list,
        "Folder peeking",
        "Preview folders on hover in Icons and List views.",
        PreferenceManager::folder_peeking,
        PreferenceManager::set_folder_peeking,
    );
    switch(
        &list,
        "Keep arrows in file list",
        "Stop arrow keys from leaving the file list. Use %{sidebar_focus_keys} to focus the sidebar, or use the mouse.",
        PreferenceManager::arrow_navigation_scoped,
        PreferenceManager::set_arrow_navigation_scoped,
    );
    switch(
        &list,
        "Show sizes in Columns view",
        "Show file sizes and folder item counts when hovering or selecting items in Columns view.",
        PreferenceManager::columns_show_sizes,
        PreferenceManager::set_columns_show_sizes,
    );
    switch(
        &list,
        "Mirror columns selection",
        "Show the selected folder's contents in the next pane as you move with the keyboard.",
        PreferenceManager::columns_mirror_selection,
        PreferenceManager::set_columns_mirror_selection,
    );

    let sorting = group.section("Sorting");
    switch(
        &sorting,
        "Remember view, sort and icon size per folder",
        "Each folder keeps the view, sort and thumbnail size you choose there. Other folders use the defaults.",
        PreferenceManager::remember_folder_views,
        PreferenceManager::set_remember_folder_views,
    );
    choice(
        &sorting,
        "Default view",
        "Folders without their own view open in this one.",
        &[
            ("Columns", BrowserMode::Columns),
            ("Icons", BrowserMode::Icons),
            ("List", BrowserMode::List),
        ],
        PreferenceManager::browser_mode,
        PreferenceManager::set_default_mode,
    );
    choice(
        &sorting,
        "Default sort",
        "Folders without their own sort use this order.",
        &[
            ("Name", SortKey::Name),
            ("Size", SortKey::Size),
            ("Modified", SortKey::Modified),
            ("Type", SortKey::Type),
        ],
        |p| p.default_sort().0,
        |p, key| p.set_default_sort(key, p.default_sort().1),
    );
    let direction = segmented(
        &[
            ("Ascending", SortDirection::Ascending),
            ("Descending", SortDirection::Descending),
        ],
        PreferenceManager::shared().default_sort().1,
        |direction| {
            let prefs = PreferenceManager::shared();
            prefs.set_default_sort(prefs.default_sort().0, direction);
        },
    );
    sorting.append(&row(
        "Default sort direction",
        "",
        Some(direction.upcast_ref()),
    ));
    let forget = gtk::Button::with_label(&crate::i18n::tr("Clear"));
    forget.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
        "Forget all folder settings",
    ))]);
    forget.connect_clicked(|_| PreferenceManager::shared().forget_folder_views());
    PreferenceManager::shared().bind_preference(
        &forget,
        PreferenceManager::has_folder_views,
        |w, saved| w.set_sensitive(saved),
    );
    sorting.append(&row(
        "Forget all folder settings",
        "Return every folder to the default view, sort and thumbnail size.",
        Some(forget.upcast_ref()),
    ));
}

fn place_visibility(id: &str) -> Option<(Read, Write)> {
    Some(match id {
        "home" => (
            PreferenceManager::sidebar_show_home,
            PreferenceManager::set_sidebar_show_home,
        ),
        "desktop" => (
            PreferenceManager::sidebar_show_desktop,
            PreferenceManager::set_sidebar_show_desktop,
        ),
        "documents" => (
            PreferenceManager::sidebar_show_documents,
            PreferenceManager::set_sidebar_show_documents,
        ),
        "downloads" => (
            PreferenceManager::sidebar_show_downloads,
            PreferenceManager::set_sidebar_show_downloads,
        ),
        "music" => (
            PreferenceManager::sidebar_show_music,
            PreferenceManager::set_sidebar_show_music,
        ),
        "pictures" => (
            PreferenceManager::sidebar_show_pictures,
            PreferenceManager::set_sidebar_show_pictures,
        ),
        "videos" => (
            PreferenceManager::sidebar_show_videos,
            PreferenceManager::set_sidebar_show_videos,
        ),
        "recent" => (
            PreferenceManager::sidebar_show_recent,
            PreferenceManager::set_sidebar_show_recent,
        ),
        "network" => (
            PreferenceManager::sidebar_show_network,
            PreferenceManager::set_sidebar_show_network,
        ),
        "trash" => (
            PreferenceManager::sidebar_show_trash,
            PreferenceManager::set_sidebar_show_trash,
        ),
        _ => return None,
    })
}

/// `order` with `moved` placed where `target` is.
fn reordered(order: &[&'static str], moved: &str, target: &str) -> Vec<&'static str> {
    let mut order = order.to_vec();
    let Some(from) = order.iter().position(|id| *id == moved).filter(|_| moved != target)
    else {
        return order;
    };
    let item = order.remove(from);
    let to = order
        .iter()
        .position(|id| *id == target)
        .map_or(order.len(), |to| if to >= from { to + 1 } else { to });
    order.insert(to, item);
    order
}

const PLACE_DRAG_PREFIX: &str = "nyx-files-place:";

/// One row per sidebar place, in sidebar order; drag a row onto another to move it.
fn place_rows(list: &gtk::Box) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let prefs = PreferenceManager::shared();
    for id in prefs.places_order() {
        let (Some((icon, name)), Some((read, write))) =
            (super::sidebar::place_identity(id), place_visibility(id))
        else {
            continue;
        };
        let toggle = gtk::Switch::new();
        toggle.set_active(read(&prefs));
        toggle.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(name))]);
        toggle.connect_active_notify(move |t| write(&PreferenceManager::shared(), t.is_active()));
        let r = row(name, "", Some(toggle.upcast_ref()));
        r.add_css_class("place-row");
        let image = gtk::Image::from_icon_name(icon);
        image.set_valign(gtk::Align::Center);
        r.prepend(&image);
        let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        handle.add_css_class("drag-handle");
        handle.set_valign(gtk::Align::Center);
        handle.set_tooltip_text(Some(&crate::i18n::tr("Drag to reorder")));
        r.prepend(&handle);
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let key = format!("{PLACE_DRAG_PREFIX}{id}");
        source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(&key.to_value()))
        });
        let dragged = r.downgrade();
        source.connect_drag_begin(move |source, _| {
            if let Some(r) = dragged.upgrade() {
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&r))), 0, 0);
            }
        });
        r.add_controller(source);
        let target = gtk::DropTarget::new(String::static_type(), gtk::gdk::DragAction::MOVE);
        target.connect_enter(|t, _, _| {
            if let Some(w) = t.widget() {
                w.add_css_class("drop-target");
            }
            gtk::gdk::DragAction::MOVE
        });
        target.connect_leave(|t| {
            if let Some(w) = t.widget() {
                w.remove_css_class("drop-target");
            }
        });
        target.connect_drop(move |t, value, _, _| {
            if let Some(w) = t.widget() {
                w.remove_css_class("drop-target");
            }
            let Some(moved) = value
                .get::<String>()
                .ok()
                .and_then(|v| v.strip_prefix(PLACE_DRAG_PREFIX).map(str::to_owned))
            else {
                return false;
            };
            gtk::glib::idle_add_local_once(move || {
                let prefs = PreferenceManager::shared();
                prefs.set_places_order(&reordered(&prefs.places_order(), &moved, id));
            });
            true
        });
        r.add_controller(target);
        prefs.bind_preference(&toggle, read, |widget, on| {
            if let Some(t) = widget.downcast_ref::<gtk::Switch>()
                && t.is_active() != on
            {
                t.set_active(on);
            }
        });
        list.append(&r);
    }
}

/// "Theme default" and the theme's swatches for a sidebar section's color,
/// stored by swatch name so a pick follows theme switches.
fn section_swatches(section: &'static str, title: &str) -> gtk::Box {
    let strip = widgets::hbox(6);
    strip.set_valign(gtk::Align::Center);
    let mut names: Vec<Option<String>> = vec![None];
    names.extend(nyx_theme::swatches().into_iter().map(|(name, _)| Some(name)));
    let title = crate::i18n::tr(title);
    let buttons: Vec<(gtk::Button, gtk::CssProvider, Option<String>)> = names
        .into_iter()
        .map(|name| {
            let button = gtk::Button::new();
            button.add_css_class("accent-swatch");
            let tip = match &name {
                None => {
                    button.add_css_class("theme-default");
                    crate::i18n::tr("Theme default")
                }
                Some(name) => {
                    let mut chars = name.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                        .unwrap_or_default()
                }
            };
            button.set_tooltip_text(Some(&tip));
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "{title}: {tip}"
            ))]);
            let provider = gtk::CssProvider::new();
            #[expect(deprecated, reason = "a per-widget colour has no non-deprecated provider")]
            button
                .style_context()
                .add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_USER);
            let picked = name.clone();
            button.connect_clicked(move |_| {
                PreferenceManager::shared().set_sidebar_section_color(section, picked.as_deref());
                super::theme::refresh_sections();
            });
            strip.append(&button);
            (button, provider, name)
        })
        .collect();
    let refresh = move || {
        let swatches = nyx_theme::swatches();
        let current = PreferenceManager::shared().sidebar_section_color(section);
        for (button, provider, name) in &buttons {
            let colour = match name {
                None => Some(super::theme::theme_section_colour(section)),
                Some(name) => swatches
                    .iter()
                    .find(|(swatch, _)| swatch == name)
                    .map(|(_, colour)| colour.clone()),
            };
            if let Some(colour) = colour {
                provider.load_from_string(&format!("* {{ background: {colour}; }}"));
            }
            if *name == current {
                button.add_css_class("selected");
            } else {
                button.remove_css_class("selected");
            }
        }
    };
    refresh();
    nyx_theme::subscribe(&strip, refresh.clone());
    PreferenceManager::shared().bind_preference(
        &strip,
        PreferenceManager::sidebar_section_colors,
        move |_, _| refresh(),
    );
    strip
}

fn sidebar(group: &Group) {
    let places = group.section("Places");
    PreferenceManager::shared().bind_preference(
        &places,
        PreferenceManager::places_order,
        |widget, _| {
            if let Some(list) = widget.downcast_ref::<gtk::Box>() {
                place_rows(list);
            }
        },
    );
    let colors = group.section("Section colors");
    for (section, title) in [
        ("places", "Places"),
        ("pinned", "Pinned"),
        ("cloud", "Cloud Storage"),
        ("devices", "Devices"),
    ] {
        let strip = section_swatches(section, title);
        strip.set_halign(gtk::Align::Start);
        let r = row(title, "", None);
        r.set_orientation(gtk::Orientation::Vertical);
        r.set_spacing(10);
        r.append(&strip);
        colors.append(&r);
    }
    switch(
        &colors,
        "Colored sidebar icons",
        "Show each sidebar icon in its section's color. Off, only the current place's icon is colored.",
        PreferenceManager::colored_sidebar_icons,
        PreferenceManager::set_colored_sidebar_icons,
    );
    cloud::append(group);
}

fn search(group: &Group) {
    let list = group.section("");
    switch(
        &list,
        "Type to search",
        "Start filtering the active pane as soon as you type.",
        PreferenceManager::type_to_search,
        PreferenceManager::set_type_to_search,
    );
    switch(
        &list,
        "Include subfolders",
        "Also match items inside nested folders.",
        PreferenceManager::filter_include_subfolders,
        PreferenceManager::set_filter_include_subfolders,
    );
    switch(
        &list,
        "Open search results directly",
        "Launch files from search instead of showing the quick preview.",
        PreferenceManager::search_open_files_directly,
        PreferenceManager::set_search_open_files_directly,
    );
    choice(
        &list,
        "Search time limit",
        "Stop indexing a search after this long. Slow or network mounts need longer to list every folder.",
        &crate::ui::preferences::SEARCH_TIME_LIMIT_CHOICES,
        PreferenceManager::search_time_limit,
        PreferenceManager::set_search_time_limit,
    );
    choice(
        &list,
        "Search entry limit",
        "Stop indexing a search after this many files and folders. Higher limits use more memory.",
        &crate::ui::preferences::SEARCH_ENTRY_LIMIT_CHOICES,
        PreferenceManager::search_entry_limit,
        PreferenceManager::set_search_entry_limit,
    );
    group
        .section("Global search exclusions")
        .append(&crate::ui::settings::exclusions::search_exclusions_control(
            &PreferenceManager::shared(),
        ));
}

fn previews(group: &Group) {
    let list = group.section("");
    let style = segmented(
        &[
            ("Popup", PreviewStyle::Popup),
            ("Panel", PreviewStyle::Panel),
        ],
        PreferenceManager::shared().preview_style(),
        |style| PreferenceManager::shared().set_preview_style(style),
    );
    list.append(&row(
        "Preview style",
        "Open file previews in a popup window or a side panel.",
        Some(style.upcast_ref()),
    ));
    switch(
        &list,
        "Single-click file previews",
        "Show a quick preview when selecting a supported file.",
        PreferenceManager::single_click_previews,
        PreferenceManager::set_single_click_previews,
    );
    switch(
        &list,
        "Autoplay media previews",
        "Start playing video, audio, and GIF previews as soon as they open.",
        PreferenceManager::preview_autoplay,
        PreferenceManager::set_preview_autoplay,
    );
    switch(
        &list,
        "Render documents by default",
        "Open Markdown and HTML previews in the rendered view instead of source.",
        PreferenceManager::render_documents_by_default,
        PreferenceManager::set_render_documents_by_default,
    );
    let thumbnails = group.section("Thumbnails");
    switch(
        &thumbnails,
        "Show thumbnails",
        "Show thumbnails instead of file-type icons. The preview panel stays available.",
        PreferenceManager::show_thumbnails,
        PreferenceManager::set_show_thumbnails,
    );
    let workers =
        gtk::SpinButton::with_range(1.0, crate::sandbox::browser::MAX_WORKERS as f64, 1.0);
    workers.set_value(PreferenceManager::shared().thumbnail_workers() as f64);
    workers.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
        "Thumbnail workers",
    ))]);
    workers.connect_value_changed(|s| {
        PreferenceManager::shared().set_thumbnail_workers(s.value() as usize)
    });
    thumbnails.append(&row(
        "Thumbnail workers",
        "Parallel thumbnail decoders across all windows. More workers use more CPU and memory.",
        Some(workers.upcast_ref()),
    ));
    switch(
        &thumbnails,
        "Hardware-accelerated video previews",
        "Decode video thumbnails on the GPU.",
        PreferenceManager::hardware_accelerated_video_previews,
        PreferenceManager::set_hardware_accelerated_video_previews,
    );
    use crate::sandbox::MediaPreviewBackend;
    let backend = gtk::DropDown::from_strings(&[&crate::i18n::tr("Automatic"), "VA-API", "Vulkan"]);
    let values = [
        MediaPreviewBackend::Automatic,
        MediaPreviewBackend::VaApi,
        MediaPreviewBackend::Vulkan,
    ];
    backend.update_property(&[gtk::accessible::Property::Label(&crate::i18n::tr(
        "Decoding backend",
    ))]);
    let current = PreferenceManager::shared().video_preview_backend();
    backend.set_selected(values.iter().position(|v| *v == current).unwrap_or(0) as u32);
    backend.connect_selected_notify(move |d| {
        if let Some(value) = values.get(d.selected() as usize) {
            PreferenceManager::shared().set_video_preview_backend(*value);
        }
    });
    PreferenceManager::shared().bind_preference(
        &backend,
        PreferenceManager::hardware_accelerated_video_previews,
        |w, on| w.set_sensitive(on),
    );
    thumbnails.append(&row("Decoding backend", "", Some(backend.upcast_ref())));
}

fn transfers(group: &Group) {
    let list = group.section("");
    choice(
        &list,
        "Drag & drop to another device",
        "What happens when you drop items onto a different drive or share.",
        &[
            ("Always copy", CrossVolumeDropStrategy::Copy),
            ("Always move", CrossVolumeDropStrategy::Move),
            ("Always ask", CrossVolumeDropStrategy::Ask),
        ],
        PreferenceManager::cross_volume_drop_strategy,
        PreferenceManager::set_cross_volume_drop_strategy,
    );
    switch(
        &list,
        "Open folder after dropping files",
        "Show the destination folder after a successful drag and drop.",
        PreferenceManager::open_folder_after_drop,
        PreferenceManager::set_open_folder_after_drop,
    );
    switch(
        &list,
        "Watch network folders",
        "Check open folders on network shares every few seconds for changes made on other computers. When off, those changes show only after a refresh.",
        PreferenceManager::watch_network_folders,
        PreferenceManager::set_watch_network_folders,
    );
}

/// The group box and the list inside one of the card's standard groups.
fn standard_list(body: &gtk::Box, name: &str) -> Option<(gtk::Box, gtk::Box)> {
    let mut child = body.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        if w.has_css_class("settings-group") && w.widget_name() == name {
            let wrapper = w.downcast::<gtk::Box>().ok()?;
            let mut inner = wrapper.first_child();
            while let Some(c) = inner {
                inner = c.next_sibling();
                if let Ok(list) = c.clone().downcast::<gtk::Box>()
                    && list.has_css_class("group-list")
                {
                    return Some((wrapper, list));
                }
            }
            return Some((wrapper.clone(), wrapper));
        }
    }
    None
}

/// Files' rows for Appearance, Keyboard and About.
fn extend_standard(body: &gtk::Box) {
    if let Some((_, list)) = standard_list(body, "Appearance") {
        let edit = widgets::icon_button(
            "document-edit-symbolic",
            &crate::i18n::tr("Edit theme file"),
        );
        edit.connect_clicked(|_| {
            let id = nyx_theme::read_suite().unwrap_or_default().theme;
            if let Some(path) = nyx_theme::edit_copy(&id) {
                nyx_core::cmd::open_in_editor(&path);
            }
        });
        list.append(&row(
            "Theme file",
            "Open the current theme in an editor; saving it restyles every Nyx app.",
            Some(edit.upcast_ref()),
        ));
        choice(
            &list,
            "Interface renderer",
            "Cairo avoids text artifacts on some displays. Restart to apply changes; GSK_RENDERER overrides this choice.",
            &[
                ("GTK default", InterfaceRenderer::System),
                ("Cairo", InterfaceRenderer::Cairo),
            ],
            PreferenceManager::interface_renderer,
            PreferenceManager::set_interface_renderer,
        );
        restart_row(
            &list,
            "Restart to apply interface renderer",
            PreferenceManager::interface_renderer_restart_required,
        );
    }
    if let Some((wrapper, list)) = standard_list(body, "Keyboard") {
        switch(
            &list,
            "10xer mode",
            crate::ui::tenxer_mode::MODE_DESCRIPTION,
            PreferenceManager::tenxer_mode,
            PreferenceManager::set_tenxer_mode,
        );
        switch(
            &list,
            "Show %{reference_keys} Shortcuts button",
            "Show the shortcuts button in the bottom bar. Item counts and clipboard status remain visible when the button is hidden. %{reference_keys} always opens the full reference.",
            PreferenceManager::show_keybinding_hints,
            PreferenceManager::set_show_keybinding_hints,
        );
        let scope = crate::ui::shortcut_reference::ReferenceScope::window(
            PreferenceManager::shared().browser_mode(),
        );
        for section in crate::ui::shortcut_reference::reference_sections(scope) {
            let shortcuts = card(&wrapper, Some(section_title(section.title)));
            for shortcut in &section.rows {
                let keys = widgets::label(&shortcut.key_text(), "key-cap");
                shortcuts.append(&plain_row(
                    &shortcut.action_text(),
                    "",
                    Some(keys.upcast_ref()),
                ));
            }
        }
    }
    if let Some((wrapper, _)) = standard_list(body, "About") {
        let build = card(&wrapper, Some(section_title("Build")));
        let kind = crate::build_info::build_kind();
        if kind != crate::services::BuildKind::Stable {
            let badge = widgets::tag(&kind.localized_label(), "accent");
            build.append(&row("Release channel", "", Some(badge.upcast_ref())));
        }
        for (title, value) in [
            ("Commit", crate::build_info::COMMIT.to_owned()),
            ("Toolkit", toolkit_version()),
        ] {
            let value = widgets::label(&value, "dim");
            value.set_selectable(true);
            build.append(&row(title, "", Some(value.upcast_ref())));
        }
        let copy = gtk::Button::with_label(&crate::i18n::tr("Copy version info"));
        copy.connect_clicked(|button| button.clipboard().set_text(&version_info()));
        build.append(&row(
            "Version info",
            "For bug reports",
            Some(copy.upcast_ref()),
        ));
        wrapper.append(&section_title("Diagnostics"));
        wrapper.append(&crate::ui::settings::diagnostics::diagnostics_page());
    }
}

#[cfg(test)]
mod tests {
    use super::reordered;

    #[test]
    fn dropping_a_place_on_another_takes_its_position() {
        let order = ["home", "desktop", "recent", "trash"];
        assert_eq!(
            reordered(&order, "home", "recent"),
            ["desktop", "recent", "home", "trash"]
        );
        assert_eq!(
            reordered(&order, "trash", "desktop"),
            ["home", "trash", "desktop", "recent"]
        );
        assert_eq!(reordered(&order, "recent", "recent"), order);
        assert_eq!(reordered(&order, "missing", "home"), order);
    }
}
