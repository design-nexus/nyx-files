// SPDX-License-Identifier: MIT

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use sourceview5::prelude::BufferExt as _;

use super::super::*;
use crate::{
    model::{
        EntryKind, FileEntry, FolderSort, Location, MetadataValue, SortDirection, SortKey,
        ViewPreferences,
    },
    services::SearchLimits,
    test_support::gtk_test,
    ui::{
        browser_modes::{BrowserDensity, BrowserMode, ClickCount, DensityScale, FrameDensity},
        preferences::fixtures::{
            non_default_preferences, seed_omarchy_colors_for_test, seed_omarchy_for_test,
            seed_saved_preferences_for_test,
        },
        theme::ThemeManager,
    },
};

#[test]
fn saved_language_applies_before_settings_and_changes_only_after_restart() {
    gtk_test(
        "ui::preferences::tests::preferences::saved_language_applies_before_settings_and_changes_only_after_restart",
        || {
            let mut preferences = non_default_preferences();
            preferences.language = crate::i18n::Language::French;
            fs::create_dir_all(settings_path().parent().expect("settings directory"))
                .expect("create settings directory");
            fs::write(
                settings_path(),
                toml::to_string(&preferences).expect("serialize settings"),
            )
            .expect("save fixture");
            let manager = PreferenceManager::load();
            assert_eq!(crate::i18n::tr("Language"), "Langue");
            assert!(!manager.language_restart_required());
            manager.set_language(crate::i18n::Language::Japanese);
            assert!(manager.language_restart_required());
            assert_eq!(crate::i18n::tr("Language"), "Langue");
            crate::storage::writer::settle();
            assert_eq!(
                read_preferences().expect("read saved language").language,
                crate::i18n::Language::Japanese
            );
            manager.set_language(crate::i18n::Language::French);
            assert!(!manager.language_restart_required());
            manager.set_language(crate::i18n::Language::Japanese);
            crate::storage::writer::settle();
            let restarted = PreferenceManager::load();
            assert!(!restarted.language_restart_required());
            assert_eq!(&*rust_i18n::locale(), "ja");
        },
    );
}


#[test]
fn older_preferences_keep_backward_compatible_behavior_defaults() {
    let mut saved = toml::Table::try_from(non_default_preferences()).expect("saved preferences");
    saved.remove("filter_include_subfolders");
    saved.remove("search_time_limit");
    saved.remove("search_entry_limit");
    saved.remove("open_folder_after_drop");
    saved.remove("date_format");
    saved.remove("send_to_recent_destinations");
    saved.remove("tenxer_mode");
    saved.remove("omarchy_variant");
    saved.remove("file_icon_colors");
    saved.remove("folder_peeking");
    saved.remove("columns_show_sizes");
    saved.remove("language");
    let restored: Preferences = saved.try_into().expect("backward-compatible preferences");
    assert_eq!(
        restored,
        Preferences {
            language: crate::i18n::Language::Auto,
            filter_include_subfolders: true,
            search_time_limit: 10,
            search_entry_limit: 200_000,
            open_folder_after_drop: false,
            date_format: "relative".into(),
            send_to_recent_destinations: HashMap::new(),
            tenxer_mode: false,
            omarchy_variant: OmarchyVariant::Original,
            file_icon_colors: FileIconColors::Brand,
            folder_peeking: false,
            columns_show_sizes: true,
            ..non_default_preferences()
        }
    );
}

#[test]
fn empty_send_to_history_is_omitted_from_saved_preferences() {
    let serialized = toml::Table::try_from(Preferences::default()).expect("default preferences");
    assert!(!serialized.contains_key("send_to_recent_destinations"));
}

#[test]
fn malformed_send_to_history_does_not_discard_other_preferences() {
    let mut saved = toml::Table::try_from(non_default_preferences()).expect("saved preferences");
    saved.insert("send_to_recent_destinations".into(), "invalid".into());
    assert!(saved.clone().try_into::<Preferences>().is_err());

    assert_eq!(
        salvage_preferences(saved),
        Preferences {
            send_to_recent_destinations: HashMap::new(),
            ..non_default_preferences()
        }
    );
}


#[test]
fn a_malformed_preference_does_not_discard_the_others() {
    let mut saved = toml::Table::try_from(non_default_preferences()).expect("saved preferences");
    saved.insert("show_hidden".into(), "yes".into());
    saved.insert("omarchy_variant".into(), "unknown".into());
    assert!(saved.clone().try_into::<Preferences>().is_err());

    assert_eq!(
        salvage_preferences(saved),
        Preferences {
            show_hidden: false,
            omarchy_variant: OmarchyVariant::Original,
            ..non_default_preferences()
        }
    );
}

#[test]
fn a_missing_required_preference_does_not_discard_the_others() {
    let mut saved = toml::Table::try_from(non_default_preferences()).expect("saved preferences");
    saved.remove("theme");
    assert!(saved.clone().try_into::<Preferences>().is_err());

    assert_eq!(
        salvage_preferences(saved),
        Preferences {
            theme: Preferences::default().theme,
            ..non_default_preferences()
        }
    );
}

fn assert_recovered_preferences_survive_save(
    corrupt: impl FnOnce(&mut toml::Table),
    mut expected: Preferences,
) {
    seed_saved_preferences_for_test();
    let mut saved = toml::Table::try_from(non_default_preferences()).expect("saved preferences");
    corrupt(&mut saved);
    let malformed = toml::to_string(&saved).expect("syntactically valid TOML");
    fs::write(settings_path(), &malformed).expect("persist malformed preferences");

    let manager = PreferenceManager::shared();
    assert_eq!(*manager.preferences.borrow(), expected);
    crate::storage::writer::settle();
    assert_eq!(
        fs::read_to_string(settings_path()).expect("unchanged settings file"),
        malformed
    );
    expected.folder_peeking = false;
    manager.set_folder_peeking(false);
    crate::storage::writer::settle();

    let saved = fs::read_to_string(settings_path()).expect("saved file");
    let persisted: Preferences = toml::from_str(&saved).expect("save repairs invalid preferences");
    assert_eq!(persisted, expected);
    let saved_keys: BTreeSet<String> = toml::from_str::<toml::Table>(&saved)
        .expect("saved TOML")
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    let known_keys: BTreeSet<String> = toml::Table::try_from(&expected)
        .expect("known preferences")
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert_eq!(
        saved_keys, known_keys,
        "a save keeps only known preferences"
    );
    crate::storage::writer::settle();
    assert_eq!(read_preferences().expect("saved preferences"), expected);
}


#[test]
fn changes_apply_at_once_and_reach_the_file_on_the_settings_writer() {
    gtk_test(
        "ui::preferences::tests::preferences::changes_apply_at_once_and_reach_the_file_on_the_settings_writer",
        || {
            seed_saved_preferences_for_test();
            let manager = PreferenceManager::shared();
            let anchors = [
                gtk::Box::new(gtk::Orientation::Vertical, 0),
                gtk::Box::new(gtk::Orientation::Vertical, 0),
            ];
            let values = Rc::new(RefCell::new(Vec::new()));
            for anchor in &anchors {
                let observed = values.clone();
                manager.bind_preference(
                    anchor,
                    PreferenceManager::folder_peeking,
                    move |_, value| {
                        observed.borrow_mut().push(value);
                    },
                );
            }
            let held = crate::storage::writer::hold();
            manager.set_folder_peeking(false);
            assert_eq!(*values.borrow(), [true, true, false, false]);
            assert!(
                read_preferences()
                    .expect("saved preferences")
                    .folder_peeking,
                "the file waits for the settings writer"
            );
            drop(held);
            crate::storage::writer::settle();
            assert!(
                !read_preferences()
                    .expect("saved preferences")
                    .folder_peeking
            );

            let held = crate::storage::writer::hold();
            manager.set_folder_peeking(true);
            let releasing = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                drop(held);
            });
            crate::storage::writer::flush_at_exit();
            assert!(
                read_preferences()
                    .expect("saved preferences")
                    .folder_peeking,
                "quitting waits for queued saves"
            );
            releasing.join().expect("released writer");
        },
    );
}

#[test]
fn missing_settings_allow_first_run_saves() {
    gtk_test(
        "ui::preferences::tests::preferences::missing_settings_allow_first_run_saves",
        || {
            assert!(!settings_path().exists());
            let manager = PreferenceManager::load();
            assert!(!manager.folder_peeking());
            manager.set_folder_peeking(true);
            crate::storage::writer::settle();
            assert!(read_preferences().expect("first run save").folder_peeking);
            crate::storage::writer::settle();
            assert!(PreferenceManager::load().folder_peeking());
            manager.set_folder_peeking(false);
            crate::storage::writer::settle();
            assert!(!PreferenceManager::load().folder_peeking());
        },
    );
}

#[test]
fn symlinked_settings_save_through_to_the_target() {
    gtk_test(
        "ui::preferences::tests::preferences::symlinked_settings_save_through_to_the_target",
        || {
            seed_saved_preferences_for_test();
            let config_home = std::env::var_os("XDG_CONFIG_HOME").expect("isolated config home");
            let target = Path::new(&config_home)
                .parent()
                .expect("sandbox root")
                .join("dotfiles/settings.toml");
            fs::create_dir_all(target.parent().expect("dotfiles directory"))
                .expect("dotfiles directory");
            fs::rename(settings_path(), &target).expect("move settings into dotfiles");
            std::os::unix::fs::symlink(&target, settings_path()).expect("dotfiles link");

            let manager = PreferenceManager::load();
            assert!(manager.folder_peeking());
            manager.set_folder_peeking(false);
            crate::storage::writer::settle();

            assert!(
                !manager.persistence_dirty.get(),
                "the save through the link must not stay pending"
            );
            assert!(
                fs::symlink_metadata(settings_path())
                    .expect("settings link")
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(
                fs::read_link(settings_path()).expect("settings link"),
                target
            );
            let saved: Preferences =
                toml::from_str(&fs::read_to_string(&target).expect("dotfiles settings"))
                    .expect("dotfiles settings parse");
            assert!(!saved.folder_peeking);
            drop(manager);
            crate::storage::writer::settle();
            assert!(!PreferenceManager::load().folder_peeking());
        },
    );
}


#[derive(Debug)]
struct SaveNoticeText {
    title: String,
    summary: String,
    detail: String,
}


fn wait_for(condition: impl Fn() -> bool, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "waiting for {what}");
        glib::MainContext::default().iteration(false);
    }
}

fn save_notices(window: &gtk::Window) -> Vec<SaveNoticeText> {
    while glib::MainContext::default().iteration(false) {}
    fn descendants(widget: &gtk::Widget, found: &mut Vec<gtk::Widget>) {
        found.push(widget.clone());
        let mut child = widget.first_child();
        while let Some(current) = child {
            descendants(&current, found);
            child = current.next_sibling();
        }
    }
    let mut widgets = Vec::new();
    descendants(window.upcast_ref(), &mut widgets);
    widgets
        .iter()
        .filter(|widget| widget.has_css_class("app-modal-layer"))
        .map(|layer| {
            let mut inner = Vec::new();
            descendants(layer, &mut inner);
            let text = |class: &str| {
                inner
                    .iter()
                    .find(|widget| widget.has_css_class(class))
                    .and_then(|widget| {
                        widget
                            .downcast_ref::<gtk::Label>()
                            .map(|label| label.text())
                    })
                    .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
                    .unwrap_or_default()
            };
            SaveNoticeText {
                title: text("action-dialog-title"),
                summary: text("action-dialog-subtitle"),
                detail: text("action-dialog-description"),
            }
        })
        .collect()
}

fn assert_notice(notice: &SaveNoticeText, title: &str, detail_start: &str) {
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    assert_eq!(notice.title, title);
    assert_eq!(notice.summary, "Changes last only until Files closes");
    assert!(
        compact(&notice.detail).starts_with(&compact(detail_start)),
        "{notice:?}"
    );
}

#[test]
fn malformed_preferences_survive_startup_and_an_unrelated_save() {
    gtk_test(
        "ui::preferences::tests::preferences::malformed_preferences_survive_startup_and_an_unrelated_save",
        || {
            assert_recovered_preferences_survive_save(
                |saved| {
                    saved.insert("show_hidden".into(), "yes".into());
                },
                Preferences {
                    show_hidden: false,
                    ..non_default_preferences()
                },
            );
        },
    );
}

#[test]
fn missing_required_preferences_survive_startup_and_an_unrelated_save() {
    gtk_test(
        "ui::preferences::tests::preferences::missing_required_preferences_survive_startup_and_an_unrelated_save",
        || {
            assert_recovered_preferences_survive_save(
                |saved| {
                    saved.remove("theme");
                },
                Preferences {
                    theme: Preferences::default().theme,
                    ..non_default_preferences()
                },
            );
        },
    );
}

#[test]
fn multiple_invalid_preferences_do_not_block_later_valid_entries() {
    gtk_test(
        "ui::preferences::tests::preferences::multiple_invalid_preferences_do_not_block_later_valid_entries",
        || {
            assert_recovered_preferences_survive_save(
                |saved| {
                    saved.insert("watch_network_folders".into(), "no".into());
                    saved.insert("hardware_accelerated_video_previews".into(), "no".into());
                    saved.insert("show_hidden".into(), "yes".into());
                    saved.insert("future_preference".into(), true.into());
                    saved.insert("auto_refresh_interval".into(), 600.into());
                },
                Preferences {
                    watch_network_folders: true,
                    hardware_accelerated_video_previews: None,
                    show_hidden: false,
                    ..non_default_preferences()
                },
            );
        },
    );
}

#[test]
fn unlisted_search_limits_round_up_on_load_and_set() {
    gtk_test(
        "ui::preferences::tests::preferences::unlisted_search_limits_round_up_on_load_and_set",
        || {
            for (stored_secs, expected_secs, stored_entries, expected_entries) in [
                (0, 0, 0, 0),
                (1, 10, 1, 200_000),
                (11, 30, 200_001, 500_000),
                (61, 300, 500_001, 1_000_000),
                (301, 900, 1_000_001, 5_000_000),
                (901, 0, 5_000_001, 0),
            ] {
                assert_recovered_preferences_survive_save(
                    |saved| {
                        saved.insert("search_time_limit".into(), i64::from(stored_secs).into());
                        saved.insert(
                            "search_entry_limit".into(),
                            i64::from(stored_entries).into(),
                        );
                    },
                    Preferences {
                        search_time_limit: expected_secs,
                        search_entry_limit: expected_entries,
                        ..non_default_preferences()
                    },
                );
            }

            seed_saved_preferences_for_test();
            let manager = PreferenceManager::shared();
            for (requested, expected) in [(45, 60), (u32::MAX, 0), (300, 300), (10, 10)] {
                manager.set_search_time_limit(requested);
                assert_eq!(manager.search_time_limit(), expected);
                crate::storage::writer::settle();
                assert_eq!(
                    read_preferences()
                        .expect("saved preferences")
                        .search_time_limit,
                    expected
                );
            }
            for (requested, expected) in [
                (300_000, 500_000),
                (u32::MAX, 0),
                (200_000, 200_000),
                (1, 200_000),
            ] {
                manager.set_search_entry_limit(requested);
                assert_eq!(manager.search_entry_limit(), expected);
                crate::storage::writer::settle();
                assert_eq!(
                    read_preferences()
                        .expect("saved preferences")
                        .search_entry_limit,
                    expected
                );
            }
            manager.set_search_time_limit(0);
            manager.set_search_entry_limit(0);
            assert_eq!(
                manager.search_limits(),
                SearchLimits {
                    time_budget: None,
                    max_entries: None,
                }
            );
        },
    );
}

#[test]
fn fresh_preferences_select_tokyo_night_before_settings_opens() {
    gtk_test(
        "ui::preferences::tests::preferences::fresh_preferences_select_tokyo_night_before_settings_opens",
        || {
            assert!(!settings_path().exists());
            let themes = ThemeManager::shared();
            let manager = PreferenceManager::shared();
            assert_eq!(themes.selected_id(), "tokyo-night");
            assert!(manager.filter_include_subfolders());
            assert!(!themes.follows_omarchy());
            assert_eq!(
                themes.current_tokens().expect("selected theme").name,
                "Tokyo Night"
            );
            assert!(!settings_path().exists());
        },
    );
}




fn assert_theme_colors(widget: &impl IsA<gtk::Widget>, tokens: &crate::ui::theme::ThemeTokens) {
    for (name, expected) in [
        ("strata_bg", &tokens.background),
        ("strata_surface", &tokens.surface),
        ("strata_accent", &tokens.accent),
        ("strata_text", &tokens.text),
    ] {
        #[expect(deprecated, reason = "GTK has no replacement for named CSS colors")]
        let actual = widget
            .style_context()
            .lookup_color(name)
            .expect("applied theme color");
        assert_eq!(
            actual,
            gtk::gdk::RGBA::parse(expected).expect("valid token"),
            "{name}"
        );
    }
}

#[test]
fn omarchy_colors_in_gtk_only_hex_forms_apply_to_css_and_icons() {
    gtk_test(
        "ui::preferences::tests::preferences::omarchy_colors_in_gtk_only_hex_forms_apply_to_css_and_icons",
        || {
            seed_omarchy_colors_for_test(
                "background = '#112233'\nforeground = '#ddeeff'\naccent = '#000aaafff'\nselection = '#1111222233338888'\n",
            );
            let themes = ThemeManager::shared();
            assert!(themes.follows_omarchy());
            let tokens = themes.appearance_tokens();
            assert_eq!(tokens.accent, "#00aaff");
            assert_eq!(tokens.highlight, "#11223388");
            let window = gtk::Window::new();
            for (name, expected) in [
                ("strata_accent", "#00aaff"),
                ("strata_highlight", "#11223388"),
            ] {
                #[expect(deprecated, reason = "GTK has no replacement for named CSS colors")]
                let applied = window.style_context().lookup_color(name);
                assert_eq!(
                    applied,
                    Some(gtk::gdk::RGBA::parse(expected).expect("canonical color")),
                    "{name}"
                );
            }
            assert_eq!(crate::assets::primary_icon_color(), "#00aaff");
            window.close();
        },
    );
}


fn wait_for_theme(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "live Omarchy refresh timed out"
        );
        glib::MainContext::default().iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}


#[test]
fn density_sliders_save_snap_points_as_presets_and_other_values_as_custom() {
    gtk_test(
        "ui::preferences::tests::preferences::density_sliders_save_snap_points_as_presets_and_other_values_as_custom",
        || {
            let manager = PreferenceManager::shared();
            assert_eq!(manager.browser_density(), BrowserDensity::Compact);
            assert_eq!(manager.frame_density(), FrameDensity::Compact);

            manager.set_browser_density(BrowserDensity::from_px(11));
            manager.set_frame_density(FrameDensity::from_px(4));
            {
                let saved = manager.preferences.borrow();
                assert_eq!(saved.browser_density, "custom");
                assert_eq!(saved.browser_density_custom_px, 11);
                assert_eq!(saved.frame_density, "airy");
            }
            assert_eq!(manager.browser_density(), BrowserDensity::Custom(11));
            assert_eq!(manager.frame_density(), FrameDensity::Airy);

            manager.set_browser_density(BrowserDensity::Custom(4));
            assert_eq!(manager.preferences.borrow().browser_density, "compact");
            assert_eq!(manager.browser_density(), BrowserDensity::Compact);
            assert_eq!(
                manager.frame_density(),
                FrameDensity::Airy,
                "content changes leave the frame alone"
            );

            manager.set_frame_density(FrameDensity::Custom(200));
            assert_eq!(manager.frame_density(), FrameDensity::Custom(12));
        },
    );
}

#[test]
fn saved_date_format_renders_before_settings_and_updates_bound_labels() {
    gtk_test(
        "ui::preferences::tests::preferences::saved_date_format_renders_before_settings_and_updates_bound_labels",
        || {
            seed_saved_preferences_for_test();
            let manager = PreferenceManager::shared();
            let seconds = glib::DateTime::now_local().expect("local time").to_unix() - 120;
            let entry = FileEntry {
                location: Location::local("/fixture/recent.txt"),
                native_name: "recent.txt".into(),
                display_name: "recent.txt".into(),
                thumbnail_path: None,
                kind: EntryKind::File,
                size: MetadataValue::Known(4),
                modified_unix_seconds: MetadataValue::Known(seconds),
                mode: MetadataValue::Known(0o100644),
                recent_unix_seconds: MetadataValue::Unknown,
                is_hidden: false,
                image_dimensions: MetadataValue::Unknown,
                child_count: MetadataValue::Unknown,
                duration_seconds: MetadataValue::Unknown,
                recent_uri: None,
            };
            let absolute = |pattern: &str| {
                glib::DateTime::from_unix_local(seconds)
                    .expect("modified date")
                    .format(pattern)
                    .expect("format")
                    .to_string()
            };
            let windows = [gtk::Window::new(), gtk::Window::new()];
            let labels: Vec<gtk::Label> = windows
                .iter()
                .map(|window| {
                    let label = gtk::Label::new(None);
                    window.set_child(Some(&label));
                    crate::util::set_modified_date(&label, Some(&entry), "—");
                    label
                })
                .collect();
            for label in &labels {
                assert_eq!(label.label(), absolute("%Y-%m-%d %H:%M"));
            }
            manager.set_date_format(crate::util::DateFormat::Long);
            for label in &labels {
                assert_eq!(label.label(), absolute("%B %-d, %Y, %H:%M"));
            }
            crate::util::set_modified_date(&labels[0], None, "unknown");
            manager.set_date_format(crate::util::DateFormat::Relative);
            assert_eq!(labels[0].label(), "unknown");
            let text = labels[1].label();
            assert!(text == "Just now" || text.ends_with(" ago"), "{text}");
            let mut rebound = entry.clone();
            rebound.modified_unix_seconds = MetadataValue::Known(seconds - 86400);
            crate::util::set_modified_date(&labels[0], Some(&rebound), "—");
            manager.set_date_format(crate::util::DateFormat::Iso8601);
            let expected = glib::DateTime::from_unix_local(seconds - 86400)
                .expect("rebound date")
                .format("%Y-%m-%d %H:%M")
                .expect("format");
            assert_eq!(labels[0].label(), expected);
            assert_eq!(labels[1].label(), absolute("%Y-%m-%d %H:%M"));
            let rebuilt = gtk::Label::new(None);
            windows[1].set_child(Some(&rebuilt));
            crate::util::set_modified_date(&rebuilt, Some(&entry), "—");
            assert_eq!(rebuilt.label(), absolute("%Y-%m-%d %H:%M"));
            manager.set_date_format(crate::util::DateFormat::Long);
            assert_eq!(rebuilt.label(), absolute("%B %-d, %Y, %H:%M"));
            for window in windows {
                window.close();
            }
        },
    );
}

#[derive(Clone, Copy)]
enum ItemStep {
    Relocate {
        from: &'static str,
        to: Option<&'static str>,
        merged: bool,
    },
    Forget {
        root: &'static str,
        trashed: Option<TrashMount>,
    },
    Restore(&'static str),
    RestoreRenumbered(&'static str),
    /// Puts a different folder at `path`, keeping the original alive elsewhere.
    ReplaceOnDisk(&'static str),
}

#[derive(Clone, Copy)]
enum TrashMount {
    KeepsNumbers,
    RenewsNumbers,
    RenewsNumbersWithoutBirths,
    RenewsNumbersInOneBirthTick,
}


#[test]
fn settings_files_are_watched_from_the_monitors_worker_without_missing_a_save() {
    gtk_test(
        "ui::preferences::tests::preferences::settings_files_are_watched_from_the_monitors_worker_without_missing_a_save",
        || {
            use crate::adapters::{FsOp, FsQuirks, Quirks};

            seed_saved_preferences_for_test();
            let folder_views = super::super::folder_view_settings::folder_views_path();
            fs::create_dir_all(folder_views.parent().expect("state directory"))
                .expect("state directory");
            fs::write(&folder_views, "").expect("folder settings file");
            let sandbox = glib::home_dir()
                .parent()
                .expect("isolated sandbox")
                .to_path_buf();
            let gate = crate::test_support::Gate::default();
            let scope = FsQuirks::install(
                &sandbox,
                Quirks {
                    before: vec![(FsOp::Watch, "settings.toml", 0, gate.racer())],
                    ..Quirks::default()
                },
            );
            let ticked = Rc::new(std::cell::Cell::new(false));
            let tick = ticked.clone();
            glib::timeout_add_local_once(Duration::from_millis(10), move || tick.set(true));

            let manager = PreferenceManager::shared();
            wait_for(
                || gate.reached() && ticked.get(),
                "the settings watch to wait on the monitors worker while the window runs",
            );
            assert_eq!(
                manager.default_sort(),
                (SortKey::Size, SortDirection::Descending)
            );
            let mut saved = non_default_preferences();
            saved.sort_key = "modified".into();
            saved.sort_direction = "ascending".into();
            fs::write(
                settings_path(),
                toml::to_string(&saved).expect("serialize another process's save"),
            )
            .expect("another process saves");
            gate.release();
            wait_for(
                || manager.default_sort() == (SortKey::Modified, SortDirection::Ascending),
                "the save made before the watch was live",
            );
            assert_eq!(scope.count(FsOp::Watch), 2);

            drop(manager);
            wait_for(
                || scope.count(FsOp::Unwatch) == 2,
                "both watches to be released on the monitors worker",
            );
        },
    );
}




#[test]
fn places_order_keeps_every_place_once_and_appends_missing_ones() {
    let saved: Vec<String> = ["trash", "bogus", "home", "trash", "recent"]
        .map(str::to_owned)
        .to_vec();
    assert_eq!(
        sanitize_places_order(&saved),
        [
            "trash",
            "home",
            "recent",
            "desktop",
            "documents",
            "downloads",
            "music",
            "pictures",
            "videos",
            "network",
        ]
    );
    assert_eq!(sanitize_places_order(&[]), PLACE_IDS);
    assert_eq!(
        sanitize_places_order(&Preferences::default().places_order),
        PLACE_IDS,
        "a new install shows the sidebar's own order"
    );
}

#[test]
fn folders_remember_their_own_view_and_follow_the_default_otherwise() {
    gtk_test(
        "ui::preferences::tests::preferences::folders_remember_their_own_view_and_follow_the_default_otherwise",
        || {
            let manager = PreferenceManager::shared();
            let photos = Location::local("/home/me/Pictures");
            let music = Location::local("/home/me/Music");
            assert!(manager.remember_folder_views());
            assert_eq!(manager.browser_mode(), BrowserMode::Columns);

            manager.set_folder_mode(Some(&photos), BrowserMode::Icons);
            assert_eq!(manager.mode_for(Some(&photos)), BrowserMode::Icons);
            assert_eq!(manager.mode_for(Some(&music)), BrowserMode::Columns);
            assert_eq!(manager.browser_mode(), BrowserMode::Columns);
            assert!(manager.has_folder_views());

            let revision = manager.folder_modes_revision();
            manager.set_default_mode(BrowserMode::List);
            assert_eq!(manager.mode_for(Some(&music)), BrowserMode::List);
            assert_eq!(manager.mode_for(Some(&photos)), BrowserMode::Icons);
            manager.set_default_mode(BrowserMode::Icons);
            assert_ne!(manager.folder_modes_revision(), revision);
            assert!(!manager.has_folder_views(), "a view equal to the default is dropped");

            manager.set_folder_mode(Some(&photos), BrowserMode::Icons);
            assert!(!manager.has_folder_views(), "the default view is not stored");
            let trash = Location::uri("trash:///");
            manager.set_folder_mode(Some(&trash), BrowserMode::List);
            assert_eq!(manager.mode_for(Some(&trash)), BrowserMode::Icons);

            manager.set_folder_mode(Some(&photos), BrowserMode::List);
            manager.set_remember_folder_views(false);
            assert_eq!(manager.mode_for(Some(&photos)), BrowserMode::Icons);
            manager.set_folder_mode(Some(&music), BrowserMode::Columns);
            assert_eq!(manager.browser_mode(), BrowserMode::Columns);
            assert_eq!(manager.mode_for(Some(&photos)), BrowserMode::Columns);
            manager.set_remember_folder_views(true);
            assert_eq!(manager.mode_for(Some(&photos)), BrowserMode::List);

            manager.flush_folder_views();
            crate::storage::writer::settle();
            let saved = fs::read_to_string(folder_view_settings::folder_views_path())
                .expect("saved folder settings");
            assert!(saved.contains("mode = \"list\""), "{saved}");
        },
    );
}

#[test]
fn preview_presentation_defaults_to_popup_and_persists_panel() {
    gtk_test("ui::preferences::tests::preferences::preview_presentation_defaults_to_popup_and_persists_panel", || {
        let manager = PreferenceManager::load();
        assert_eq!(manager.preview_style(), PreviewStyle::Popup);
        manager.set_preview_style(PreviewStyle::Panel);
        crate::storage::writer::settle();
        assert_eq!(PreferenceManager::load().preview_style(), PreviewStyle::Panel);
        let mut saved = toml::Value::try_from(manager.preferences.borrow().clone()).expect("preferences");
        saved.as_table_mut().unwrap().remove("preview_style");
        fs::write(settings_path(), toml::to_string(&saved).unwrap()).unwrap();
        assert_eq!(PreferenceManager::load().preview_style(), PreviewStyle::Popup);
        saved.as_table_mut().unwrap().insert("preview_style".into(), toml::Value::String("unknown".into()));
        fs::write(settings_path(), toml::to_string(&saved).unwrap()).unwrap();
        assert_eq!(PreferenceManager::load().preview_style(), PreviewStyle::Popup);
    });
}
