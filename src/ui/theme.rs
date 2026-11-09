// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gtk::{gdk, gio, glib, prelude::*};
use serde::{Deserialize, Serialize};
use sourceview5::prelude::BufferExt as _;

use super::browser_modes::DensityScale;
use super::preferences::{
    CornerRoundness, FileIconColors, OmarchyVariant, PreferenceManager, TextSize,
    desktop_text_scale_factor, notify_live, snapped_root_font_px,
};
use crate::{adapters::Watched, model::Location, services::LoadHandle};

mod font;
mod omarchy;

/// Above GTK's user priority, every property is unset and the built-in theme
/// restated, so `~/.config/gtk-4.0/gtk.css` cannot decide any of them.
const RESET_STYLE_PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_USER + 1;
const BASE_THEME_STYLE_PRIORITY: u32 = RESET_STYLE_PRIORITY + 1;
/// Below Strata's stylesheet, so its intentional monospace rules still win.
const FONT_STYLE_PRIORITY: u32 = BASE_THEME_STYLE_PRIORITY + 1;
const STYLE_PRIORITY: u32 = FONT_STYLE_PRIORITY + 1;
/// Above the static stylesheet so generated density spacing wins over its presets.
const DENSITY_STYLE_PRIORITY: u32 = STYLE_PRIORITY + 1;
const TOKEN_STYLE_PRIORITY: u32 = DENSITY_STYLE_PRIORITY + 1;
/// Above every Files stylesheet, so `~/.config/strata/style.css` has the final word.
const USER_STYLE_PRIORITY: u32 = TOKEN_STYLE_PRIORITY + 1;
const USER_STYLE_FILE: &str = "style.css";

thread_local! {
    static SHARED_MANAGER: RefCell<Option<Rc<ThemeManager>>> = const { RefCell::new(None) };
    static SOURCE_STYLE_PATH_INSTALLED: Cell<bool> = const { Cell::new(false) };
    static SOURCE_LANGUAGE_PATH_INSTALLED: Cell<bool> = const { Cell::new(false) };
    static SOURCE_BUFFERS: RefCell<Vec<glib::WeakRef<sourceview5::Buffer>>> = const { RefCell::new(Vec::new()) };
    static DOCUMENT_BUFFERS: RefCell<Vec<glib::WeakRef<gtk::TextBuffer>>> = const { RefCell::new(Vec::new()) };
    static DOCUMENT_VIEWS: RefCell<Vec<glib::WeakRef<super::document_view::DocumentTextView>>> = const { RefCell::new(Vec::new()) };
    /// Installed on the first source preview buffer, so startup performs no SourceView I/O.
    static PENDING_STYLE_TOKENS: RefCell<Option<(ThemeTokens, Option<SourcePalette>)>> = const { RefCell::new(None) };
    static STYLE_SCHEME_DIRTY: Cell<bool> = const { Cell::new(true) };
    static SOURCE_STYLE_RESOURCE: RefCell<Option<gio::Resource>> = const { RefCell::new(None) };
}

const SOURCE_STYLE_FOLDER: &str = "/io/github/design_nexus/nyx/Files/generated/source-styles/";


#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ThemeTokens {
    pub name: String,
    pub background: String,
    pub surface: String,
    pub text: String,
    pub accent: String,
    #[serde(default = "default_danger")]
    pub danger: String,
    pub muted: String,
    pub highlight: String,
    pub border: String,
    pub dim_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_keyword: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_constant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax_preprocessor: Option<String>,
}

#[derive(Clone)]
struct SourcePalette {
    statement: String,
    string: String,
    constant: String,
    type_color: String,
    preprocessor: String,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub id: String,
    pub tokens: ThemeTokens,
    #[cfg(test)]
    pub custom: bool,
}

/// Appearance preferences that change the shared CSS or icon rendering the manager applies.
#[derive(Clone, Debug, PartialEq)]
struct AppearancePreferences {
    mode: String,
    theme: String,
    omarchy_variant: OmarchyVariant,
    text_size: TextSize,
    corner_roundness: CornerRoundness,
    interface_font: Option<String>,
    element_glow: bool,
    file_icon_colors: FileIconColors,
}

type ThemeRefreshPreference = dyn Fn(&gtk::Widget, &ThemeManager);

struct ThemePreferenceListener {
    active: Cell<bool>,
    anchor: glib::WeakRef<gtk::Widget>,
    refresh: Box<ThemeRefreshPreference>,
}

#[derive(Default)]
struct ThemeListeners {
    notifying: Cell<bool>,
    listeners: Rc<RefCell<Vec<Rc<ThemePreferenceListener>>>>,
}

impl ThemeListeners {
    fn bind<T: PartialEq + Clone + 'static>(
        &self,
        manager: &ThemeManager,
        anchor: &impl IsA<gtk::Widget>,
        read: impl Fn(&ThemeManager) -> T + 'static,
        apply: impl Fn(&gtk::Widget, T) + 'static,
    ) {
        let previous = RefCell::new(None);
        let listener = Rc::new(ThemePreferenceListener {
            active: Cell::new(true),
            anchor: anchor.as_ref().downgrade(),
            refresh: Box::new(move |widget, manager| {
                let value = read(manager);
                if previous.borrow().as_ref() == Some(&value) {
                    return;
                }
                previous.replace(Some(value.clone()));
                apply(widget, value);
            }),
        });
        self.listeners.borrow_mut().push(listener.clone());
        let weak_listeners = Rc::downgrade(&self.listeners);
        let weak_listener = Rc::downgrade(&listener);
        anchor.connect_destroy(move |_| {
            if let Some(listener) = weak_listener.upgrade() {
                listener.active.set(false);
            }
            if let Some(listeners) = weak_listeners.upgrade() {
                listeners.borrow_mut().retain(|candidate| {
                    !std::rc::Weak::ptr_eq(&Rc::downgrade(candidate), &weak_listener)
                });
            }
        });
        (listener.refresh)(anchor.as_ref(), manager);
    }

    fn notify(&self, manager: &ThemeManager) {
        if self.notifying.replace(true) {
            return;
        }
        notify_live(
            &self.listeners,
            |listener| listener.active.get() && listener.anchor.upgrade().is_some(),
            |listener| {
                if listener.active.get()
                    && let Some(anchor) = listener.anchor.upgrade()
                {
                    (listener.refresh)(&anchor, manager);
                }
            },
        );
        self.notifying.set(false);
    }
}

pub struct ThemeManager {
    provider: gtk::CssProvider,
    #[cfg(test)]
    preview_generation: Cell<u64>,
    reset_provider: gtk::CssProvider,
    base_theme_provider: gtk::CssProvider,
    font_provider: gtk::CssProvider,
    base_provider: gtk::CssProvider,
    density_provider: gtk::CssProvider,
    density: Cell<
        Option<(
            super::browser_modes::BrowserDensity,
            super::browser_modes::FrameDensity,
        )>,
    >,
    user_provider: gtk::CssProvider,
    base_roundness: Cell<Option<CornerRoundness>>,
    themes: RefCell<Vec<Theme>>,
    preferences: Rc<PreferenceManager>,
    omarchy_available: Cell<bool>,
    omarchy_state: RefCell<Option<OmarchyState>>,
    omarchy_monitors: RefCell<Vec<(PathBuf, LoadHandle)>>,
    pending_omarchy_refresh: RefCell<Option<glib::SourceId>>,
    themes_monitor: RefCell<Option<LoadHandle>>,
    pending_themes_refresh: RefCell<Option<glib::SourceId>>,
    user_style_monitor: RefCell<Option<LoadHandle>>,
    pending_user_style_refresh: RefCell<Option<glib::SourceId>>,
    preview: RefCell<Option<ThemeTokens>>,
    /// Omarchy state can be unreadable mid-switch while the window keeps these colors.
    applied_omarchy_tokens: RefCell<Option<ThemeTokens>>,
    appearance: RefCell<AppearancePreferences>,
    base_variant_dark: Cell<Option<bool>>,
    theme_listeners: ThemeListeners,
    active_model_palette: Cell<crate::services::ModelPalette>,
}

impl ThemeManager {
    pub fn shared() -> Rc<Self> {
        let existing = SHARED_MANAGER.with(|shared| shared.borrow().clone());
        if let Some(manager) = existing {
            return manager;
        }
        let manager = Self::load();
        SHARED_MANAGER.with(|shared| shared.replace(Some(manager.clone())));
        manager
    }

    fn load() -> Rc<Self> {
        let preferences = PreferenceManager::shared();
        let themes = merge_builtin_and_custom_themes(builtins(), load_custom_themes());
        let omarchy = OmarchyRefresh::read();
        let omarchy_available = omarchy
            .state
            .as_ref()
            .and_then(|state| state.tokens(OmarchyVariant::Original))
            .is_some();
        let suite = nyx_theme::read_suite().unwrap_or_default();
        let appearance = AppearancePreferences {
            mode: suite_mode(&suite),
            theme: suite.theme.clone(),
            omarchy_variant: preferences.omarchy_variant(),
            text_size: preferences.text_size(),
            corner_roundness: preferences.corner_roundness(),
            interface_font: preferences.interface_font(),
            element_glow: preferences.element_glow(),
            file_icon_colors: preferences.file_icon_colors(),
        };

        let manager = Rc::new(Self {
            #[cfg(test)]
            preview_generation: Cell::new(0),
            provider: gtk::CssProvider::new(),
            reset_provider: gtk::CssProvider::new(),
            base_theme_provider: gtk::CssProvider::new(),
            font_provider: gtk::CssProvider::new(),
            base_provider: gtk::CssProvider::new(),
            density_provider: gtk::CssProvider::new(),
            density: Cell::new(None),
            user_provider: gtk::CssProvider::new(),
            base_roundness: Cell::new(None),
            themes: RefCell::new(themes),
            preferences: preferences.clone(),
            omarchy_available: Cell::new(omarchy_available),
            omarchy_state: RefCell::new(omarchy.state),
            omarchy_monitors: RefCell::new(Vec::new()),
            pending_omarchy_refresh: RefCell::new(None),
            themes_monitor: RefCell::new(None),
            pending_themes_refresh: RefCell::new(None),
            user_style_monitor: RefCell::new(None),
            pending_user_style_refresh: RefCell::new(None),
            preview: RefCell::new(None),
            applied_omarchy_tokens: RefCell::new(None),
            appearance: RefCell::new(appearance),
            base_variant_dark: Cell::new(None),
            theme_listeners: ThemeListeners::default(),
            active_model_palette: Cell::new(crate::services::ModelPalette {
                accent: 0,
                surface: 0,
            }),
        });
        let weak = Rc::downgrade(&manager);
        preferences.observe(Rc::new(move || {
            if let Some(manager) = weak.upgrade() {
                manager.on_preferences_changed();
            }
        }));
        pin_manual_font_rendering();
        manager.install_user_stylesheet();
        manager.apply_selected();
        manager.monitor_text_scaling();
        manager.monitor_omarchy(&omarchy.directories);
        manager.monitor_custom_themes(themes_directory().is_dir());
        manager.follow_suite();
        manager
            .user_provider
            .load_from_string(&read_user_stylesheet());
        manager.monitor_user_stylesheet();
        manager
    }


    pub fn is_omarchy_available(&self) -> bool {
        self.omarchy_available.get()
    }

    pub fn follows_omarchy(&self) -> bool {
        nyx_theme::read_suite().unwrap_or_default().mode == nyx_theme::ThemeMode::Omarchy
    }

    pub fn selected_id(&self) -> String {
        nyx_theme::read_suite().unwrap_or_default().theme
    }

    pub(crate) fn active_model_palette(&self) -> crate::services::ModelPalette {
        self.active_model_palette.get()
    }

    /// Applies the current value immediately, then only changes to that value.
    /// Capture weak references to owned widgets/state; the anchor owns the binding's lifetime.
    pub(crate) fn bind_theme_preference<T: PartialEq + Clone + 'static>(
        &self,
        anchor: &impl IsA<gtk::Widget>,
        read: impl Fn(&Self) -> T + 'static,
        apply: impl Fn(&gtk::Widget, T) + 'static,
    ) {
        self.theme_listeners.bind(self, anchor, read, apply);
    }

    #[cfg(test)]
    /// Applies `tokens` until [`Self::cancel_preview`] or a theme selection; appearance
    /// changes in the meantime re-apply the preview rather than the saved theme.
    /// Returns the preview's generation for [`Self::cancel_preview_from`].
    pub fn preview(&self, tokens: &ThemeTokens) -> Option<u64> {
        validate_tokens(tokens).ok()?;
        self.preview.replace(Some(tokens.clone()));
        self.preview_generation
            .set(self.preview_generation.get().wrapping_add(1));
        self.apply_tokens(tokens, None);
        Some(self.preview_generation.get())
    }

    #[cfg(test)]
    pub fn cancel_preview(&self) {
        if self.preview.take().is_some() {
            self.apply_selected();
        }
    }

    #[cfg(test)]
    /// Cancels the preview only if no later one has replaced it.
    pub fn cancel_preview_from(&self, generation: u64) {
        if self.preview_generation.get() == generation {
            self.cancel_preview();
        }
    }

    #[cfg(test)]
    pub fn starter_tokens(&self) -> ThemeTokens {
        self.current_tokens().unwrap_or_else(azure_tokens)
    }




    #[cfg(test)]
    pub(crate) fn is_previewing(&self) -> bool {
        self.preview.borrow().is_some()
    }

    pub fn appearance_tokens(&self) -> ThemeTokens {
        suite_tokens()
    }

    /// The user's own stylesheet in Files' config folder has the final word.
    fn install_user_stylesheet(&self) {
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &self.user_provider,
                USER_STYLE_PRIORITY,
            );
        }
        self.user_provider
            .connect_parsing_error(|_, section, error| {
                tracing::warn!(
                    %error,
                    path = %user_stylesheet_path().display(),
                    location = %section,
                    "invalid rule in user stylesheet"
                );
            });
    }

    /// A missing or unreadable file clears the provider, so deleting
    /// `style.css` restores Strata's own appearance without a restart.
    fn refresh_user_stylesheet(self: &Rc<Self>) {
        let read = crate::storage::writer::run(read_user_stylesheet);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            if let (Ok(css), Some(manager)) = (read.await, weak.upgrade()) {
                manager.user_provider.load_from_string(&css);
            }
        });
    }

    fn monitor_user_stylesheet(self: &Rc<Self>) {
        let (changed, live) = (Rc::downgrade(self), Rc::downgrade(self));
        let monitor = crate::adapters::watch(
            &Location::local(user_stylesheet_path()),
            Watched::File,
            move |_, _| {
                if let Some(manager) = changed.upgrade() {
                    manager.schedule_user_style_refresh();
                }
            },
            move || {
                if let Some(manager) = live.upgrade() {
                    manager.schedule_user_style_refresh();
                }
            },
        );
        self.user_style_monitor.replace(Some(monitor));
    }

    fn schedule_user_style_refresh(self: &Rc<Self>) {
        if let Some(pending) = self.pending_user_style_refresh.borrow_mut().take() {
            pending.remove();
        }
        let weak = Rc::downgrade(self);
        let refresh = glib::timeout_add_local_once(Duration::from_millis(75), move || {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            manager.pending_user_style_refresh.borrow_mut().take();
            manager.refresh_user_stylesheet();
        });
        self.pending_user_style_refresh.replace(Some(refresh));
    }





    fn apply_selected(&self) {
        let preview = self.preview.borrow().clone();
        if let Some(tokens) = preview {
            self.apply_tokens(&tokens, None);
            return;
        }
        let tokens = suite_tokens();
        self.applied_omarchy_tokens.replace(Some(tokens.clone()));
        self.apply_tokens(&tokens, None);
    }

    /// Another app's theme or accent choice, a saved theme file, or Omarchy changing.
    fn follow_suite(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let last = RefCell::new((suite_tokens(), theme_list_stamp()));
        glib::timeout_add_seconds_local(1, move || {
            let Some(manager) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let now = (suite_tokens(), theme_list_stamp());
            if *last.borrow() != now {
                let themes_changed = last.borrow().1 != now.1;
                last.replace(now);
                if themes_changed {
                    manager.themes.replace(merge_builtin_and_custom_themes(builtins(), load_custom_themes()));
                }
                manager.suite_changed();
            }
            glib::ControlFlow::Continue
        });
    }

    fn suite_changed(&self) {
        if self.preview.borrow().is_none() {
            self.apply_selected();
        }
        self.preferences.notify_changes();
    }

    fn omarchy_tokens(&self, variant: OmarchyVariant) -> Option<ThemeTokens> {
        self.omarchy_state
            .borrow()
            .as_ref()
            .and_then(|state| state.tokens(variant))
    }

    pub(in crate::ui) fn current_tokens(&self) -> Option<ThemeTokens> {
        let id = self.selected_id();
        self.themes
            .borrow()
            .iter()
            .find(|theme| theme.id == id)
            .map(|theme| theme.tokens.clone())
    }

    fn apply_tokens(&self, tokens: &ThemeTokens, source_palette: Option<&SourcePalette>) {
        let color = |value: &str| {
            let [red, green, blue] = parse_rgb_channels(value).unwrap_or_default();
            (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
        };
        self.active_model_palette
            .set(crate::services::ModelPalette {
                accent: color(&tokens.accent),
                surface: color(&tokens.surface),
            });
        super::document_media::apply_theme(tokens);
        super::browser::find::apply_theme(&tokens.accent, &tokens.background);
        super::preview::audio::apply_theme(tokens);
        let root_font_px = snapped_root_font_px(
            scaled_root_font_px(self.preferences.text_size().root_font_px()),
            desktop_text_scale_factor(),
        );
        let glow = "transparent";
        self.provider.load_from_string(&format!(
            "{}\n@define-color strata_glow {glow};\n{}",
            tokens_css(tokens, root_font_px),
            suite_sizes_css()
        ));
        let family = nyx_theme::suite_theme("files").look.font.ui;
        self.font_provider
            .load_from_string(&font::stylesheet(family.as_deref()));
        apply_interface_font(family.as_deref(), root_font_px);
        let dark_background =
            parse_rgb_channels(&tokens.background).is_none_or(|[red, green, blue]| {
                u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114 < 128_000
            });
        if self.base_variant_dark.replace(Some(dark_background)) != Some(dark_background) {
            self.base_theme_provider
                .load_from_string(&base_theme_import(dark_background));
        }
        crate::assets::set_catppuccin_flavor(crate::assets::CatppuccinFlavor::for_dark_background(
            dark_background,
        ));
        crate::assets::set_catppuccin_theme_colors(
            nyx_theme::suite_theme("files").look.file_icon_colors == nyx_theme::FileIconColors::Theme,
        );
        // SVG strokes accept fewer color forms than GTK does.
        crate::assets::set_primary_icon_color(&color_to_hex(&tokens.accent));
        crate::assets::set_muted_icon_color(&color_to_hex(&tokens.dim_text));
        super::thumbnail::refresh_all_customized_icons();
        stage_source_style_scheme(tokens, source_palette);
        style_document_buffers(tokens);
        style_document_views(tokens);
        self.theme_listeners.notify(self);
    }

    fn on_preferences_changed(&self) {
        let suite = nyx_theme::read_suite().unwrap_or_default();
        let appearance = AppearancePreferences {
            mode: suite_mode(&suite),
            theme: suite.theme.clone(),
            omarchy_variant: self.preferences.omarchy_variant(),
            text_size: self.preferences.text_size(),
            corner_roundness: self.preferences.corner_roundness(),
            interface_font: self.preferences.interface_font(),
            element_glow: self.preferences.element_glow(),
            file_icon_colors: self.preferences.file_icon_colors(),
        };
        if *self.appearance.borrow() != appearance {
            self.appearance.replace(appearance);
            self.apply_selected();
        }
        self.theme_listeners.notify(self);
    }

    fn monitor_text_scaling(self: &Rc<Self>) {
        let Some(settings) = gtk::Settings::default() else {
            return;
        };
        let weak = Rc::downgrade(self);
        settings.connect_gtk_xft_dpi_notify(move |_| {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            manager.apply_selected();
        });
    }

    fn monitor_omarchy(self: &Rc<Self>, directories: &[PathBuf]) {
        let mut monitors = self.omarchy_monitors.take();
        monitors.retain(|(path, _)| directories.contains(path));
        for path in directories {
            if monitors.iter().any(|(watched, _)| watched == path) {
                continue;
            }
            let (changed, live) = (Rc::downgrade(self), Rc::downgrade(self));
            let monitor = crate::adapters::watch(
                &Location::local(path),
                Watched::Folder,
                move |file, other_file| {
                    if !is_omarchy_theme_event(file)
                        && !other_file.is_some_and(is_omarchy_theme_event)
                    {
                        return;
                    }
                    if let Some(manager) = changed.upgrade() {
                        manager.schedule_omarchy_refresh();
                    }
                },
                move || {
                    if let Some(manager) = live.upgrade() {
                        manager.schedule_omarchy_refresh();
                    }
                },
            );
            monitors.push((path.clone(), monitor));
        }
        self.omarchy_monitors.replace(monitors);
    }

    fn schedule_omarchy_refresh(self: &Rc<Self>) {
        if let Some(pending) = self.pending_omarchy_refresh.borrow_mut().take() {
            pending.remove();
        }
        let weak = Rc::downgrade(self);
        let refresh = glib::timeout_add_local_once(Duration::from_millis(75), move || {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            manager.pending_omarchy_refresh.borrow_mut().take();
            manager.refresh_omarchy_state();
        });
        self.pending_omarchy_refresh.replace(Some(refresh));
    }

    pub(in crate::ui) fn refresh_omarchy_state(self: &Rc<Self>) {
        let read = crate::storage::writer::run(OmarchyRefresh::read);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            if let (Ok(refresh), Some(manager)) = (read.await, weak.upgrade()) {
                manager.apply_omarchy_refresh(refresh);
            }
        });
    }

    fn apply_omarchy_refresh(self: &Rc<Self>, refresh: OmarchyRefresh) {
        self.monitor_omarchy(&refresh.directories);
        let unchanged = self.omarchy_state.replace(refresh.state.clone()) == refresh.state;
        let available = self.omarchy_tokens(OmarchyVariant::Original).is_some()
            || (self.is_omarchy_available() && refresh.name_exists);
        let availability_changed = self.omarchy_available.replace(available) != available;
        if !available && self.follows_omarchy() {
            // The suite falls back to its chosen theme by itself.
            self.preview.take();
            self.suite_changed();
            return;
        }
        if availability_changed {
            self.preferences.notify_changes();
            return;
        }
        if !unchanged && self.follows_omarchy() {
            self.apply_selected();
            self.preferences.notify_changes();
        }
    }

    fn monitor_custom_themes(self: &Rc<Self>, directory_exists: bool) {
        if !directory_exists || self.themes_monitor.borrow().is_some() {
            return;
        }
        let (changed, live) = (Rc::downgrade(self), Rc::downgrade(self));
        let monitor = crate::adapters::watch(
            &Location::local(themes_directory()),
            Watched::Folder,
            move |file, other_file| {
                if !is_custom_theme_event(file) && !other_file.is_some_and(is_custom_theme_event) {
                    return;
                }
                if let Some(manager) = changed.upgrade() {
                    manager.schedule_themes_refresh();
                }
            },
            move || {
                if let Some(manager) = live.upgrade() {
                    manager.schedule_themes_refresh();
                }
            },
        );
        self.themes_monitor.replace(Some(monitor));
    }

    fn schedule_themes_refresh(self: &Rc<Self>) {
        if let Some(pending) = self.pending_themes_refresh.borrow_mut().take() {
            pending.remove();
        }
        let weak = Rc::downgrade(self);
        let refresh = glib::timeout_add_local_once(Duration::from_millis(75), move || {
            let Some(manager) = weak.upgrade() else {
                return;
            };
            manager.pending_themes_refresh.borrow_mut().take();
            manager.refresh_custom_themes();
        });
        self.pending_themes_refresh.replace(Some(refresh));
    }

    pub(in crate::ui) fn refresh_custom_themes(self: &Rc<Self>) {
        let loaded = crate::storage::writer::run(load_custom_themes);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            if let (Ok(custom), Some(manager)) = (loaded.await, weak.upgrade()) {
                manager.adopt_custom_themes(custom);
            }
        });
    }

    /// A selected file caught mid-write, invalid, or removed keeps its last
    /// valid colors until a read succeeds again.
    fn adopt_custom_themes(&self, custom: Vec<Theme>) {
        let selected = self.selected_id();
        let before = self.current_tokens();
        let mut themes = merge_builtin_and_custom_themes(builtins(), custom);
        if !themes.iter().any(|theme| theme.id == selected)
            && let Some(previous) = self
                .themes
                .borrow()
                .iter()
                .find(|theme| theme.id == selected)
                .cloned()
        {
            themes.push(previous);
        }
        self.themes.replace(themes);
        let after = self.current_tokens();
        if self.follows_omarchy() || after.is_none() || after == before {
            return;
        }
        self.apply_selected();
        self.preferences.notify_changes();
    }
}

fn is_custom_theme_event(file: &gio::File) -> bool {
    file.path().is_some_and(|path| {
        path.parent() == Some(themes_directory().as_path())
            && path
                .extension()
                .is_some_and(|extension| extension == "toml")
    })
}

fn is_omarchy_theme_event(file: &gio::File) -> bool {
    file.path().is_some_and(|path| {
        let state = omarchy_state_dir();
        state.starts_with(&path)
            || path == state.join("theme")
            || path == state.join("theme.name")
            || path == state.join("theme/colors.toml")
    })
}

fn builtins() -> Vec<Theme> {
    suite_themes(false)
}

fn suite_themes(custom: bool) -> Vec<Theme> {
    let mut themes: Vec<_> = nyx_theme::all()
        .into_iter()
        .filter(|theme| theme.id.starts_with("custom:") == custom)
        .map(|theme| Theme {
            tokens: tokens_from(&theme),
            id: theme.id,
            #[cfg(test)]
            custom,
        })
        .collect();
    themes.sort_by_key(|theme| theme.tokens.name.to_lowercase());
    themes
}

fn suite_tokens() -> ThemeTokens {
    tokens_from(&nyx_theme::suite_theme("files"))
}

/// Dim text carries the same contrast floors as body text, and the lift stops
/// at the first passing step, so it stays dimmer than text.
fn tokens_from(theme: &nyx_theme::Theme) -> ThemeTokens {
    let (palette, syntax) = (&theme.palette, &theme.look.syntax);
    let hex = |value: &str| canonical_color(value).unwrap_or_else(|| value.to_owned());
    let code = |value: &Option<String>| value.as_deref().map(hex);
    let mut tokens = ThemeTokens {
        name: theme.name.clone(),
        background: hex(&palette.bg),
        surface: hex(&palette.surface),
        text: hex(&palette.text),
        accent: hex(&palette.accent),
        danger: hex(&palette.danger),
        muted: hex(&palette.muted),
        highlight: hex(&palette.highlight),
        border: hex(&palette.border),
        dim_text: hex(&palette.dim_text),
        syntax_keyword: code(&syntax.keyword),
        syntax_string: code(&syntax.string),
        syntax_constant: code(&syntax.constant),
        syntax_type: code(&syntax.type_name),
        syntax_preprocessor: code(&syntax.preprocessor),
    };
    let dim = omarchy::readable(&tokens.dim_text, &[&tokens.background], &tokens.text, 4.5);
    tokens.dim_text = omarchy::readable(&dim, &[&tokens.surface], &tokens.text, 3.0);
    tokens
}

fn suite_theme_file(tokens: &ThemeTokens) -> String {
    let mut file = toml::Table::new();
    let mut put = |key: &str, value: &str| {
        file.insert(key.to_owned(), toml::Value::String(value.to_owned()));
    };
    put("name", &tokens.name);
    put("bg", &tokens.background);
    put("surface", &tokens.surface);
    put("text", &tokens.text);
    put("dim_text", &tokens.dim_text);
    put("accent", &tokens.accent);
    put("glow", &tokens.accent);
    put("border", &tokens.border);
    put("muted", &tokens.muted);
    put("highlight", &tokens.highlight);
    put("danger", &tokens.danger);
    put("shadow", &blend(&tokens.background, "#000000", 0.5));
    let light = omarchy::contrast(&tokens.background, "#000000") > omarchy::contrast(&tokens.background, "#ffffff");
    file.insert("light".to_owned(), toml::Value::Boolean(light));
    toml::to_string_pretty(&file).unwrap_or_default()
}

fn theme_list_stamp() -> Vec<(PathBuf, Option<std::time::SystemTime>)> {
    let mut files: Vec<_> = nyx_theme::theme_dirs()
        .iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| (entry.path(), entry.metadata().and_then(|m| m.modified()).ok()))
        .collect();
    files.sort();
    files
}

/// The suite density's sizes from STYLE.md (compact, balanced, relaxed), loaded
/// after the token sizing so they win over Files' own.
fn suite_sizes_css() -> String {
    let density = nyx_theme::read_suite().unwrap_or_default().density;
    let (header, nav, row, control, status) = match density.as_str() {
        "compact" => (42, 30, 30, 30, 26),
        "relaxed" => (50, 38, 42, 34, 30),
        _ => (46, 34, 36, 32, 28),
    };
    let grouped = control - 4;
    format!(
        "headerbar, headerbar > windowhandle > box, headerbar.browser-header, headerbar.browser-header > windowhandle > box {{ min-height: {header}px; }}\n\
         headerbar button.header-action, headerbar menubutton.header-action > button, headerbar .sidebar-toggle {{ min-width: {control}px; min-height: {control}px; }}\n\
         .header-row .list-navigation-button, .header-row .pane-actions button, .header-row .pane-actions menubutton > button, .header-row .header-actions button.header-action, .header-row .header-actions menubutton.header-action > button {{ min-width: {grouped}px; min-height: {grouped}px; }}\n\
         .location-control, .location-entry, .header-row button.header-search {{ min-height: {control}px; }}\n\
         .sidebar-row {{ min-height: {nav}px; }}\n\
         .list-row {{ min-height: {row}px; }}\n\
         .shortcut-footer {{ min-height: {status}px; }}\n"
    )
}

/// Files' zoom (Ctrl+plus and minus) on top of Settings' suite-wide text size.
pub(in crate::ui) fn scaled_root_font_px(zoom_px: u32) -> u32 {
    let scale = nyx_theme::read_suite().map_or(1.0, |suite| suite.text_scale).clamp(0.5, 2.0);
    (f64::from(zoom_px) * scale).round() as u32
}

fn suite_mode(suite: &nyx_theme::Suite) -> String {
    match suite.mode {
        nyx_theme::ThemeMode::Omarchy => "omarchy".to_owned(),
        nyx_theme::ThemeMode::Theme => "theme".to_owned(),
    }
}

fn merge_builtin_and_custom_themes(mut builtins: Vec<Theme>, custom: Vec<Theme>) -> Vec<Theme> {
    builtins.retain(|builtin| !custom.iter().any(|theme| theme.id == builtin.id));
    builtins.extend(custom);
    builtins
}

fn load_custom_themes() -> Vec<Theme> {
    move_old_custom_themes();
    suite_themes(true)
}

/// Themes saved by an older Files move to the suite's theme folder; a file of
/// the same name already there is left alone.
fn move_old_custom_themes() {
    let old = super::preferences::config_directory().join("themes");
    let Ok(entries) = fs::read_dir(&old) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let (Some(name), Ok(source)) = (path.file_name(), fs::read_to_string(&path)) else {
            continue;
        };
        let Ok(tokens) = toml::from_str::<ThemeTokens>(&source) else {
            continue;
        };
        let target = themes_directory().join(name);
        if validate_tokens(&tokens).is_err() || target.exists() {
            continue;
        }
        if fs::create_dir_all(themes_directory()).is_ok()
            && crate::storage::atomic_write_config(&target, suite_theme_file(&tokens).as_bytes()).is_ok()
        {
            let _ = fs::rename(&path, path.with_extension("toml.moved"));
        }
    }
}

#[derive(Clone, PartialEq)]
struct OmarchyState {
    name: String,
    colors: String,
}

impl OmarchyState {
    fn tokens(&self, variant: OmarchyVariant) -> Option<ThemeTokens> {
        tokens_from_quattro(self.name.trim(), &self.colors, variant)
    }

}

struct OmarchyRefresh {
    state: Option<OmarchyState>,
    name_exists: bool,
    /// Ancestor watches survive moving away or replacing the current state tree.
    directories: Vec<PathBuf>,
}

impl OmarchyRefresh {
    fn read() -> Self {
        let state = omarchy_state_dir();
        let home = glib::home_dir();
        let theme = state.join("theme");
        let directories = std::iter::once(theme.as_path())
            .chain(state.ancestors().take_while(|path| path.starts_with(&home)))
            .filter(|path| path.is_dir())
            .map(Path::to_path_buf)
            .collect();
        let read = |name: &str| fs::read_to_string(state.join(name)).ok();
        Self {
            state: read("theme.name")
                .zip(read("theme/colors.toml"))
                .map(|(name, colors)| OmarchyState { name, colors }),
            name_exists: state.join("theme.name").is_file(),
            directories,
        }
    }
}

fn tokens_from_quattro(name: &str, source: &str, variant: OmarchyVariant) -> Option<ThemeTokens> {
    let values: toml::Value = toml::from_str(source).ok()?;
    let get = |key: &str| quattro_color(&values, key);
    let source_background = get("background")?;
    let text = get("foreground")?;
    let accent = get("accent")?;
    let selection = get("selection").unwrap_or_else(|| accent.clone());
    let shadow = get("color8").unwrap_or_else(|| source_background.clone());
    let mut tokens = ThemeTokens {
        name: title_case_slug(name),
        background: blend(&source_background, &shadow, 0.35),
        surface: blend(&source_background, &shadow, 0.65),
        muted: blend(&shadow, &text, 0.10),
        highlight: selection,
        border: blend(&shadow, &text, 0.36),
        dim_text: blend(&source_background, &text, 0.62),
        text,
        accent,
        danger: get("color1").unwrap_or_else(default_danger),
        syntax_keyword: get("magenta").or_else(|| get("color5")),
        syntax_string: get("green").or_else(|| get("color2")),
        syntax_constant: get("orange").or_else(|| get("color9")),
        syntax_type: get("cyan").or_else(|| get("color3")),
        syntax_preprocessor: get("yellow"),
    };
    if variant != OmarchyVariant::Original {
        let palette = source_palette_from_quattro(source);
        omarchy::apply_variant(&mut tokens, &source_background, variant, palette.as_ref());
    }
    Some(tokens)
}

fn source_palette_from_quattro(source: &str) -> Option<SourcePalette> {
    let values: toml::Value = toml::from_str(source).ok()?;
    let get = |key: &str| quattro_color(&values, key);
    Some(SourcePalette {
        statement: get("magenta").or_else(|| get("blue"))?,
        string: get("green")?,
        constant: get("orange").or_else(|| get("yellow"))?,
        type_color: get("cyan").or_else(|| get("blue"))?,
        preprocessor: get("yellow")?,
    })
}

/// Quattro values reach GTK CSS, GtkSourceView XML and SVG strokes, so a value GTK
/// cannot parse is treated as missing; callers decide whether that is fatal.
fn quattro_color(values: &toml::Value, key: &str) -> Option<String> {
    canonical_color(values.get(key)?.as_str()?)
}

fn default_danger() -> String {
    "#e5484d".to_owned()
}

fn validate_tokens(tokens: &ThemeTokens) -> Result<(), &'static str> {
    if tokens.name.trim().is_empty() {
        return Err("Enter a theme name");
    }
    if tokens
        .color_fields()
        .any(|color| gdk::RGBA::parse(color).is_err())
    {
        return Err("Every color must be a valid CSS color");
    }
    Ok(())
}

fn source_style_manager() -> sourceview5::StyleSchemeManager {
    let manager = sourceview5::StyleSchemeManager::default();
    SOURCE_STYLE_PATH_INSTALLED.with(|installed| {
        if !installed.replace(true) {
            manager.set_search_path(&[format!("resource://{SOURCE_STYLE_FOLDER}").as_str()]);
        }
    });
    manager
}

pub(super) fn source_languages() -> sourceview5::LanguageManager {
    let manager = sourceview5::LanguageManager::default();
    SOURCE_LANGUAGE_PATH_INSTALLED.with(|installed| {
        if !installed.replace(true) {
            let user = [glib::user_data_dir(), glib::home_dir()];
            let system: Vec<_> = manager
                .search_path()
                .into_iter()
                .filter(|folder| {
                    !user.iter().any(|root| {
                        root != Path::new("/") && Path::new(folder.as_str()).starts_with(root)
                    })
                })
                .collect();
            manager.set_search_path(&system.iter().map(glib::GString::as_str).collect::<Vec<_>>());
        }
    });
    manager
}

pub(super) fn new_source_buffer() -> sourceview5::Buffer {
    let manager = source_style_manager();
    ensure_source_style_scheme_installed();
    let buffer = sourceview5::Buffer::new(None);
    buffer.set_style_scheme(manager.scheme("strata-current").as_ref());
    SOURCE_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        buffers.retain(|buffer| buffer.upgrade().is_some());
        let weak = glib::WeakRef::new();
        weak.set(Some(&buffer));
        buffers.push(weak);
    });
    buffer
}

pub(super) fn register_document_buffer(buffer: &gtk::TextBuffer) {
    let tokens = ThemeManager::shared().appearance_tokens();
    style_document_buffer(buffer, &tokens);
    DOCUMENT_BUFFERS.with(|buffers| {
        let mut buffers = buffers.borrow_mut();
        buffers.retain(|buffer| buffer.upgrade().is_some());
        let weak = glib::WeakRef::new();
        weak.set(Some(buffer));
        buffers.push(weak);
    });
}

pub(super) fn register_document_view(view: &super::document_view::DocumentTextView) {
    let tokens = ThemeManager::shared().appearance_tokens();
    style_document_view(view, &tokens);
    DOCUMENT_VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|view| view.upgrade().is_some());
        let weak = glib::WeakRef::new();
        weak.set(Some(view));
        views.push(weak);
    });
}

fn style_document_buffers(tokens: &ThemeTokens) {
    DOCUMENT_BUFFERS.with(|buffers| {
        buffers.borrow_mut().retain(|buffer| {
            let Some(buffer) = buffer.upgrade() else {
                return false;
            };
            style_document_buffer(&buffer, tokens);
            true
        });
    });
}

fn style_document_views(tokens: &ThemeTokens) {
    DOCUMENT_VIEWS.with(|views| {
        views.borrow_mut().retain(|view| {
            let Some(view) = view.upgrade() else {
                return false;
            };
            style_document_view(&view, tokens);
            true
        });
    });
}

fn style_document_view(view: &super::document_view::DocumentTextView, tokens: &ThemeTokens) {
    if let (Ok(fill), Ok(border), Ok(selection)) = (
        gdk::RGBA::parse(blend(&tokens.surface, &tokens.muted, 0.54)),
        gdk::RGBA::parse(blend(&tokens.surface, &tokens.border, 0.5)),
        gdk::RGBA::parse(&tokens.accent),
    ) {
        view.set_colors(fill, border, selection);
    }
}

fn style_document_buffer(buffer: &gtk::TextBuffer, tokens: &ThemeTokens) {
    let parse = |value: &str| gdk::RGBA::parse(value).ok();
    let table = buffer.tag_table();
    if let Some(color) = parse(&tokens.accent) {
        for name in ["document-accent", "document-link"] {
            if let Some(tag) = table.lookup(name) {
                tag.set_foreground_rgba(Some(&color));
            }
        }
    }
    if let Some(color) = parse(&tokens.dim_text)
        && let Some(tag) = table.lookup("document-dim")
    {
        tag.set_foreground_rgba(Some(&color));
    }
    if let Some(color) = parse(&tokens.text)
        && let Some(tag) = table.lookup("document-link-hover")
    {
        tag.set_foreground_rgba(Some(&color));
    }
    if let Some(color) = parse(&tokens.background)
        && let Some(tag) = table.lookup("document-selection")
    {
        tag.set_foreground_rgba(Some(&color));
    }
    if let Some(mut color) = parse(&tokens.highlight) {
        color.set_alpha(0.36);
        if let Some(tag) = table.lookup("document-quote") {
            tag.set_paragraph_background_rgba(Some(&color));
        }
        color.set_alpha(0.5);
        if let Some(tag) = table.lookup("document-link-hover") {
            tag.set_background_rgba(Some(&color));
        }
    }
}

fn stage_source_style_scheme(tokens: &ThemeTokens, source_palette: Option<&SourcePalette>) {
    PENDING_STYLE_TOKENS
        .with(|pending| pending.replace(Some((tokens.clone(), source_palette.cloned()))));
    STYLE_SCHEME_DIRTY.with(|dirty| dirty.set(true));
    let live = SOURCE_BUFFERS.with(|buffers| {
        buffers
            .borrow_mut()
            .retain(|buffer| buffer.upgrade().is_some());
        !buffers.borrow().is_empty()
    });
    if live {
        ensure_source_style_scheme_installed();
    }
}

/// Installs the staged scheme from memory and rescans the style manager, once per staged token set.
fn ensure_source_style_scheme_installed() {
    if !STYLE_SCHEME_DIRTY.with(Cell::get) {
        return;
    }
    let pending = PENDING_STYLE_TOKENS.with(|pending| pending.borrow().clone());
    let Some((tokens, palette)) = pending else {
        return;
    };
    let value = source_style_scheme_xml(&tokens, palette.as_ref());
    match crate::util::gresource::with_file(
        SOURCE_STYLE_FOLDER,
        "strata-current.xml",
        value.as_bytes(),
    ) {
        Ok(resource) => {
            STYLE_SCHEME_DIRTY.with(|dirty| dirty.set(false));
            install_source_style_scheme(resource);
        }
        Err(error) => tracing::warn!(%error, "unable to build preview syntax style"),
    }
}

fn install_source_style_scheme(resource: gio::Resource) {
    gio::resources_register(&resource);
    if let Some(previous) =
        SOURCE_STYLE_RESOURCE.with(|installed| installed.replace(Some(resource)))
    {
        gio::resources_unregister(&previous);
    }
    let manager = source_style_manager();
    manager.force_rescan();
    let scheme = manager.scheme("strata-current");
    SOURCE_BUFFERS.with(|buffers| {
        buffers.borrow_mut().retain(|buffer| {
            let Some(buffer) = buffer.upgrade() else {
                return false;
            };
            buffer.set_style_scheme(scheme.as_ref());
            true
        });
    });
}

fn resolved_source_palette(tokens: &ThemeTokens, palette: Option<&SourcePalette>) -> SourcePalette {
    let fallback = SourcePalette {
        statement: tokens.accent.clone(),
        string: blend(&tokens.accent, &tokens.text, 0.48),
        constant: blend(&tokens.accent, &tokens.text, 0.18),
        type_color: blend(&tokens.accent, &tokens.text, 0.24),
        preprocessor: blend(&tokens.accent, &tokens.text, 0.32),
    };
    let palette = palette.unwrap_or(&fallback);
    let resolve = |token: &Option<String>, fallback: &str| {
        token
            .as_deref()
            .filter(|value| gdk::RGBA::parse(*value).is_ok())
            .unwrap_or(fallback)
            .to_owned()
    };
    SourcePalette {
        statement: resolve(&tokens.syntax_keyword, &palette.statement),
        string: resolve(&tokens.syntax_string, &palette.string),
        constant: resolve(&tokens.syntax_constant, &palette.constant),
        type_color: resolve(&tokens.syntax_type, &palette.type_color),
        preprocessor: resolve(&tokens.syntax_preprocessor, &palette.preprocessor),
    }
}

impl ThemeTokens {
    #[cfg(test)]
    pub(super) fn initialize_syntax_colors(&mut self) {
        let palette = resolved_source_palette(self, None);
        self.syntax_keyword = Some(palette.statement);
        self.syntax_string = Some(palette.string);
        self.syntax_constant = Some(palette.constant);
        self.syntax_type = Some(palette.type_color);
        self.syntax_preprocessor = Some(palette.preprocessor);
    }

    // Exhaustive destructuring makes a new field fail to compile until it is classified.
    fn color_fields(&self) -> impl Iterator<Item = &String> {
        let Self {
            name: _,
            background,
            surface,
            text,
            accent,
            danger,
            muted,
            highlight,
            border,
            dim_text,
            syntax_keyword,
            syntax_string,
            syntax_constant,
            syntax_type,
            syntax_preprocessor,
        } = self;
        [
            background, surface, text, accent, danger, muted, highlight, border, dim_text,
        ]
        .into_iter()
        .chain(
            [
                syntax_keyword,
                syntax_string,
                syntax_constant,
                syntax_type,
                syntax_preprocessor,
            ]
            .into_iter()
            .flatten(),
        )
    }

}

fn source_style_scheme_xml(tokens: &ThemeTokens, palette: Option<&SourcePalette>) -> String {
    let palette = resolved_source_palette(tokens, palette);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<style-scheme id="strata-current" _name="Files Current Theme" version="1.0">
  <color name="background" value="{}"/>
  <color name="surface" value="{}"/>
  <color name="text" value="{}"/>
  <color name="accent" value="{}"/>
  <color name="danger" value="{}"/>
  <color name="selection" value="{}"/>
  <color name="dim" value="{}"/>
  <color name="string" value="{}"/>
  <color name="constant" value="{}"/>
  <color name="type" value="{}"/>
  <color name="statement" value="{}"/>
  <color name="preprocessor" value="{}"/>
  <style name="text" foreground="text" background="surface"/>
  <style name="selection" foreground="background" background="accent"/>
  <style name="cursor" foreground="accent"/>
  <style name="current-line" background="background"/>
  <style name="line-numbers" foreground="dim" background="background"/>
  <style name="def:comment" foreground="dim" italic="true"/>
  <style name="def:shebang" foreground="dim" bold="true"/>
  <style name="def:string" foreground="string"/>
  <style name="def:constant" foreground="constant"/>
  <style name="def:special-char" foreground="constant"/>
  <style name="def:identifier" foreground="text"/>
  <style name="def:statement" foreground="statement" bold="true"/>
  <style name="def:type" foreground="type" bold="true"/>
  <style name="def:preprocessor" foreground="preprocessor"/>
  <style name="def:heading" foreground="accent" bold="true"/>
  <style name="def:link-destination" foreground="string" underline="single"/>
  <style name="def:error" foreground="background" background="danger" bold="true"/>
</style-scheme>
"#,
        color_to_hex(&tokens.background),
        color_to_hex(&tokens.surface),
        color_to_hex(&tokens.text),
        color_to_hex(&tokens.accent),
        color_to_hex(&tokens.danger),
        color_to_hex(&tokens.highlight),
        color_to_hex(&tokens.dim_text),
        color_to_hex(&palette.string),
        color_to_hex(&palette.constant),
        color_to_hex(&palette.type_color),
        color_to_hex(&palette.statement),
        color_to_hex(&palette.preprocessor),
    )
}

/// GTK's automatic font rendering hints glyph outlines but not their metrics,
/// so GPU glyph atlas slots are sized from unhinted extents and lose the top
/// pixel row at some sizes (GNOME/gtk#8395). Manual rendering hints metrics
/// too and follows the desktop's hinting settings. Application values outrank
/// the settings portal, so this holds for the whole process.
fn pin_manual_font_rendering() {
    if let Some(settings) = gtk::Settings::default() {
        set_enum_setting(&settings, "gtk-font-rendering", "manual");
    }
}




/// GTK's built-in theme, restated above the reset.
const BASE_THEME: &str = "Default";



fn base_theme_import(dark: bool) -> String {
    let variant = if dark { "gtk-dark.css" } else { "gtk.css" };
    format!("@import url(\"resource:///org/gtk/libgtk/theme/{BASE_THEME}/{variant}\");")
}

/// Skips properties this GTK lacks.
fn set_enum_setting(settings: &gtk::Settings, property: &str, nick: &str) {
    let Some(spec) = settings.find_property(property) else {
        return;
    };
    if let Some(value) =
        glib::EnumClass::with_type(spec.value_type()).and_then(|class| class.to_value_by_nick(nick))
    {
        settings.set_property(property, value);
    }
}

fn apply_interface_font(family: Option<&str>, root_font_px: f64) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_font_name(Some(&font::font_name(family, root_font_px)));
    }
}


fn tokens_css(tokens: &ThemeTokens, root_font_px: f64) -> String {
    let scale = root_font_px / 13.0;
    let header = (40.0 * scale).round();
    // Column headers add six pixels of padding and three extra border pixels.
    let column_header = header - 9.0;
    let control = (24.0 * scale).round();
    // Sidebar icons are centred in control-sized boxes; headings start where the icons do.
    let sidebar_heading_inset = (control - (17.0 * scale).round()) / 2.0;
    let browser_header = (control + 8.0).max(36.0);
    // Miller headers keep their three-pixel indicator, bottom border and balancing inset.
    let browser_column_header = browser_header - 6.0;
    let sizing = format!(
        "headerbar, headerbar > windowhandle > box, .mode-pane-header, .preview-header {{ min-height: {header}px; }}\n.column-header {{ min-height: {column_header}px; }}\nheaderbar .sidebar-toggle, button.header-action, headerbar menubutton.header-action > button, .preview-header-action, button.column-header-action, menubutton.column-header-action > button {{ min-width: {control}px; min-height: {control}px; }}\n.file-operation-card button.progress-card-action {{ min-width: 0; min-height: 0; }}\n.tab-strip, headerbar.browser-header, headerbar.browser-header > windowhandle > box, .mode-pane-header, .preview-header {{ min-height: {browser_header}px; }}\n.directory-column .column-header {{ min-height: {browser_column_header}px; }}\n.mode-pane-header .list-navigation-button, .sidebar-row > box > image, .sidebar-device-action {{ min-width: {control}px; min-height: {control}px; }}\n.sidebar-heading {{ padding-left: {sidebar_heading_inset}px; }}\n"
    );
    let colors = format!(
        "@define-color strata_bg {};\n@define-color strata_surface {};\n@define-color strata_text {};\n@define-color strata_accent {};\n@define-color strata_danger {};\n@define-color strata_muted {};\n@define-color strata_highlight {};\n@define-color strata_border {};\n@define-color strata_dim_text {};\nwindow, popover, popover.background {{ font-size: {root_font_px:.6}px; }}\n",
        tokens.background,
        tokens.surface,
        tokens.text,
        tokens.accent,
        tokens.danger,
        tokens.muted,
        tokens.highlight,
        tokens.border,
        tokens.dim_text,
    );
    colors + &sizing
}

/// Parses colours GTK accepts (`#rgb`, `#rrggbb`, `rgb(...)`, names) into 8-bit
/// channels. Files emits these channels as `#rrggbb` in GtkSourceView schemes.
pub(crate) fn parse_rgb_channels(value: &str) -> Option<[u8; 3]> {
    parse_rgba_channels(value).map(|[red, green, blue, _]| [red, green, blue])
}

fn parse_rgba_channels(value: &str) -> Option<[u8; 4]> {
    let color = gdk::RGBA::parse(value).ok()?;
    let channel = |component: f32| (f64::from(component).clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([
        channel(color.red()),
        channel(color.green()),
        channel(color.blue()),
        channel(color.alpha()),
    ])
}

/// Rewrites a color GTK can parse as `#rrggbb`, or `#rrggbbaa` when translucent.
/// `gdk::RGBA::parse` also accepts 9-, 12- and 16-digit hex, which GTK CSS rejects.
fn canonical_color(value: &str) -> Option<String> {
    let [red, green, blue, alpha] = parse_rgba_channels(value)?;
    let rgb = hex_from_channels([red, green, blue]);
    Some(if alpha == u8::MAX {
        rgb
    } else {
        format!("{rgb}{alpha:02x}")
    })
}

fn hex_from_channels(channels: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", channels[0], channels[1], channels[2])
}

/// Canonicalizes a colour token to Strata's `#rrggbb` scheme representation.
pub(crate) fn color_to_hex(value: &str) -> String {
    parse_rgb_channels(value)
        .map(hex_from_channels)
        .unwrap_or_else(|| value.to_owned())
}

fn blend(left: &str, right: &str, amount: f64) -> String {
    let (Some(left), Some(right)) = (parse_rgb_channels(left), parse_rgb_channels(right)) else {
        return right.to_owned();
    };
    let channel = |index: usize| {
        let a = f64::from(left[index]);
        let b = f64::from(right[index]);
        (a + (b - a) * amount).round() as u32
    };
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}


fn title_case_slug(slug: &str) -> String {
    slug.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn themes_directory() -> PathBuf {
    nyx_theme::theme_dirs()[0].clone()
}

fn user_stylesheet_path() -> PathBuf {
    super::preferences::config_directory().join(USER_STYLE_FILE)
}

fn read_user_stylesheet() -> String {
    let path = user_stylesheet_path();
    match fs::read_to_string(&path) {
        Ok(css) => css,
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                tracing::warn!(%error, path = %path.display(), "cannot read user stylesheet");
            }
            String::new()
        }
    }
}

pub(in crate::ui) fn omarchy_state_dir() -> PathBuf {
    gtk::glib::home_dir().join(".local/state/omarchy/current")
}



#[cfg(test)]
fn azure_tokens() -> ThemeTokens {
    builtins()
        .into_iter()
        .find(|theme| theme.id == "azure-glow")
        .map(|theme| theme.tokens)
        .unwrap_or_else(|| ThemeTokens {
            name: "Azure Glow".to_owned(),
            background: "#0c1a2b".to_owned(),
            surface: "#122438".to_owned(),
            text: "#c9deed".to_owned(),
            accent: "#4fd6ff".to_owned(),
            danger: default_danger(),
            muted: "#1e3a52".to_owned(),
            highlight: "#244d68".to_owned(),
            border: "#315b75".to_owned(),
            dim_text: "#6f8da3".to_owned(),
            syntax_keyword: None,
            syntax_string: None,
            syntax_constant: None,
            syntax_type: None,
            syntax_preprocessor: None,
        })
}

#[cfg(test)]
/// File id for a new custom theme: the ASCII slug of `name`, or a generic id when the
/// name has no ASCII letters or digits, suffixed until `taken` no longer matches.
fn custom_theme_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let slug = slugify(name);
    let base = if slug.is_empty() {
        FALLBACK_CUSTOM_THEME_ID
    } else {
        slug.as_str()
    };
    let mut id = base.to_owned();
    let mut suffix = 2;
    while taken(&id) {
        id = format!("{base}-{suffix}");
        suffix += 1;
    }
    id
}

#[cfg(test)]
fn slugify(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .fold(String::new(), |mut slug, character| {
            if character.is_ascii_alphanumeric() {
                slug.push(character);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
            slug
        })
        .trim_end_matches('-')
        .to_owned()
}

#[cfg(test)]
const FALLBACK_CUSTOM_THEME_ID: &str = "custom-theme";

#[cfg(test)]
mod tests;
