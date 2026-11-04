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

const SPEC: Spec = Spec {
    groups: GROUPS,
    search_text: None,
    standard: Some(Standard {
        app: "files",
        about: About {
            name: "Files",
            icon: crate::APPLICATION_ID,
            version: env!("CARGO_PKG_VERSION"),
            description: "A fast, keyboard-first file manager.",
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
        let title = gtk::Label::new(Some(&name.to_uppercase()));
        title.add_css_class("group-title");
        title.set_xalign(0.0);
        wrapper.append(&title);
        body.append(&wrapper);
        Group { wrapper }
    }

    fn section(&self, title: &str) -> gtk::Box {
        if !title.is_empty() {
            self.wrapper.append(&section_title(title));
        }
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.add_css_class("group-list");
        self.wrapper.append(&list);
        list
    }
}

/// A small uppercase heading inside a group, as the embedded pages use.
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
    actions
        .wrapper
        .append(&crate::ui::settings::actions::actions_page());
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
        "Remember sort and icon size per folder",
        "Each folder keeps the sort and thumbnail size you choose there. Other folders use the defaults.",
        PreferenceManager::remember_folder_views,
        PreferenceManager::set_remember_folder_views,
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
        "Return every folder to the default sort and thumbnail size.",
        Some(forget.upcast_ref()),
    ));
}

fn sidebar(group: &Group) {
    let list = group.section("Places");
    let places: [(&str, Read, Write); 10] = [
        (
            "Recent",
            PreferenceManager::sidebar_show_recent,
            PreferenceManager::set_sidebar_show_recent,
        ),
        (
            "Home",
            PreferenceManager::sidebar_show_home,
            PreferenceManager::set_sidebar_show_home,
        ),
        (
            "Desktop",
            PreferenceManager::sidebar_show_desktop,
            PreferenceManager::set_sidebar_show_desktop,
        ),
        (
            "Documents",
            PreferenceManager::sidebar_show_documents,
            PreferenceManager::set_sidebar_show_documents,
        ),
        (
            "Downloads",
            PreferenceManager::sidebar_show_downloads,
            PreferenceManager::set_sidebar_show_downloads,
        ),
        (
            "Music",
            PreferenceManager::sidebar_show_music,
            PreferenceManager::set_sidebar_show_music,
        ),
        (
            "Pictures",
            PreferenceManager::sidebar_show_pictures,
            PreferenceManager::set_sidebar_show_pictures,
        ),
        (
            "Videos",
            PreferenceManager::sidebar_show_videos,
            PreferenceManager::set_sidebar_show_videos,
        ),
        (
            "Network",
            PreferenceManager::sidebar_show_network,
            PreferenceManager::set_sidebar_show_network,
        ),
        (
            "Trash",
            PreferenceManager::sidebar_show_trash,
            PreferenceManager::set_sidebar_show_trash,
        ),
    ];
    let chips = gtk::FlowBox::new();
    chips.set_selection_mode(gtk::SelectionMode::None);
    chips.set_row_spacing(6);
    chips.set_column_spacing(6);
    chips.set_max_children_per_line(10);
    chips.add_css_class("place-chips");
    for (name, read, write) in places {
        let chip = gtk::ToggleButton::with_label(&crate::i18n::tr(name));
        chip.add_css_class("chip");
        chip.set_active(read(&PreferenceManager::shared()));
        chip.connect_toggled(move |c| write(&PreferenceManager::shared(), c.is_active()));
        chips.append(&chip);
    }
    let r = row(
        "Items shown in sidebar",
        "Toggle which locations appear in the sidebar.",
        None,
    );
    r.set_orientation(gtk::Orientation::Vertical);
    r.set_spacing(10);
    r.append(&chips);
    list.append(&r);
    switch(
        &list,
        "Colored sidebar icons",
        "Show every sidebar place's icon in its color. Off, only the current place's icon is colored.",
        PreferenceManager::colored_sidebar_icons,
        PreferenceManager::set_colored_sidebar_icons,
    );
    group
        .wrapper
        .append(&crate::ui::settings::cloud::cloud_page(
            PreferenceManager::shared(),
        ));
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
    let exclusions = group.section("Global search exclusions");
    let editor =
        crate::ui::settings::exclusions::search_exclusions_control(&PreferenceManager::shared());
    exclusions.append(&row(
        "Global search exclusions",
        "Exclude folder names or directory paths from global search.",
        None,
    ));
    exclusions.append(&editor);
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

/// The list inside one of the card's standard groups.
fn standard_list(body: &gtk::Box, name: &str) -> Option<gtk::Box> {
    let mut child = body.first_child();
    while let Some(w) = child {
        child = w.next_sibling();
        if w.has_css_class("settings-group") && w.widget_name() == name {
            let mut inner = w.first_child();
            while let Some(c) = inner {
                inner = c.next_sibling();
                if let Ok(list) = c.clone().downcast::<gtk::Box>()
                    && list.has_css_class("group-list")
                {
                    return Some(list);
                }
            }
            return w.downcast::<gtk::Box>().ok();
        }
    }
    None
}

/// Files' rows for Appearance, Keyboard and About.
fn extend_standard(body: &gtk::Box) {
    if let Some(list) = standard_list(body, "Appearance") {
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
    if let Some(list) = standard_list(body, "Keyboard") {
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
            list.append(&section_title(section.title));
            for shortcut in &section.rows {
                let keys = widgets::label(&shortcut.key_text(), "key-cap");
                list.append(&widgets::row(
                    &shortcut.action_text(),
                    "",
                    Some(keys.upcast_ref()),
                ));
            }
        }
    }
    if let Some(list) = standard_list(body, "About") {
        let diagnostics = crate::ui::settings::diagnostics::diagnostics_page();
        list.append(&section_title("Diagnostics"));
        list.append(&diagnostics);
    }
}
