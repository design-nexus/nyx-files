// SPDX-License-Identifier: MIT

//! The theme colours Files draws with itself: the preview's syntax scheme,
//! rendered documents, the audio player, the 3D model palette, and the user's
//! own stylesheet. The interface's styles come from nyx-theme.

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

use super::preferences::{PreferenceManager, notify_live};
use crate::{adapters::Watched, model::Location, services::LoadHandle};

mod contrast;
mod font;

/// Above nyx-theme's stylesheets, so the user's `style.css` has the final word.
const USER_STYLE_PRIORITY: u32 = gtk::STYLE_PROVIDER_PRIORITY_USER + 8;
const USER_STYLE_FILE: &str = "style.css";
/// The preview's GtkSourceView scheme, rebuilt from the theme's colours.
const SOURCE_SCHEME: &str = "files-current";

thread_local! {
    static SHARED_MANAGER: RefCell<Option<Rc<ThemeManager>>> = const { RefCell::new(None) };
    static SOURCE_STYLE_PATH_INSTALLED: Cell<bool> = const { Cell::new(false) };
    static SOURCE_LANGUAGE_PATH_INSTALLED: Cell<bool> = const { Cell::new(false) };
    static SOURCE_BUFFERS: RefCell<Vec<glib::WeakRef<sourceview5::Buffer>>> = const { RefCell::new(Vec::new()) };
    static DOCUMENT_BUFFERS: RefCell<Vec<glib::WeakRef<gtk::TextBuffer>>> = const { RefCell::new(Vec::new()) };
    static DOCUMENT_VIEWS: RefCell<Vec<glib::WeakRef<super::document_view::DocumentTextView>>> = const { RefCell::new(Vec::new()) };
    /// Installed on the first source preview buffer, so startup performs no SourceView I/O.
    static PENDING_STYLE_TOKENS: RefCell<Option<ThemeTokens>> = const { RefCell::new(None) };
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
    /// The colours last applied.
    tokens: RefCell<ThemeTokens>,
    user_provider: gtk::CssProvider,
    user_style_monitor: RefCell<Option<LoadHandle>>,
    pending_user_style_refresh: RefCell<Option<glib::SourceId>>,
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
        let manager = Rc::new(Self {
            tokens: RefCell::new(suite_tokens()),
            user_provider: gtk::CssProvider::new(),
            user_style_monitor: RefCell::new(None),
            pending_user_style_refresh: RefCell::new(None),
            theme_listeners: ThemeListeners::default(),
            active_model_palette: Cell::new(crate::services::ModelPalette {
                accent: 0,
                surface: 0,
            }),
        });
        pin_manual_font_rendering();
        manager.install_user_stylesheet();
        manager.apply_tokens(&suite_tokens());
        manager
            .user_provider
            .load_from_string(&read_user_stylesheet());
        manager.monitor_user_stylesheet();
        let weak = Rc::downgrade(&manager);
        nyx_theme::on_change(move || {
            if let Some(manager) = weak.upgrade() {
                manager.apply_tokens(&suite_tokens());
            }
        });
        manager
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

    pub fn appearance_tokens(&self) -> ThemeTokens {
        self.tokens.borrow().clone()
    }

    /// Recolours with `tokens` as if the suite's theme had changed to them.
    #[cfg(test)]
    pub(crate) fn apply_for_test(&self, tokens: &ThemeTokens) {
        self.apply_tokens(tokens);
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
    /// `style.css` restores Files' own appearance without a restart.
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

    /// Recolours what Files draws itself with the suite's current theme.
    fn apply_tokens(&self, tokens: &ThemeTokens) {
        self.tokens.replace(tokens.clone());
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
        super::preview::audio::apply_theme(tokens);
        let family = nyx_theme::suite_theme("files").look.font.ui;
        let root_font_px = f64::from(scaled_root_font_px(
            PreferenceManager::shared().text_size().root_font_px(),
        ));
        apply_interface_font(family.as_deref(), root_font_px);
        crate::assets::set_primary_icon_color(&color_to_hex(&tokens.accent));
        crate::assets::set_muted_icon_color(&color_to_hex(&tokens.dim_text));
        super::thumbnail::refresh_all_customized_icons();
        stage_source_style_scheme(tokens);
        style_document_buffers(tokens);
        style_document_views(tokens);
        self.theme_listeners.notify(self);
    }
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
    let dim = contrast::readable(&tokens.dim_text, &[&tokens.background], &tokens.text, 4.5);
    tokens.dim_text = contrast::readable(&dim, &[&tokens.surface], &tokens.text, 3.0);
    tokens
}

/// Files' zoom (Ctrl+plus and minus) on top of Settings' suite-wide text size.
pub(in crate::ui) fn scaled_root_font_px(zoom_px: u32) -> u32 {
    let scale = nyx_theme::read_suite().map_or(1.0, |suite| suite.text_scale).clamp(0.5, 2.0);
    (f64::from(zoom_px) * scale).round() as u32
}

#[cfg(test)]
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

fn default_danger() -> String {
    "#e5484d".to_owned()
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
    buffer.set_style_scheme(manager.scheme(SOURCE_SCHEME).as_ref());
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

fn stage_source_style_scheme(tokens: &ThemeTokens) {
    PENDING_STYLE_TOKENS.with(|pending| pending.replace(Some(tokens.clone())));
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
    let Some(tokens) = pending else {
        return;
    };
    let value = source_style_scheme_xml(&tokens, None);
    match crate::util::gresource::with_file(
        SOURCE_STYLE_FOLDER,
        &format!("{SOURCE_SCHEME}.xml"),
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
    let scheme = manager.scheme(SOURCE_SCHEME);
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

}

fn source_style_scheme_xml(tokens: &ThemeTokens, palette: Option<&SourcePalette>) -> String {
    let palette = resolved_source_palette(tokens, palette);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<style-scheme id="{SOURCE_SCHEME}" _name="Files Current Theme" version="1.0">
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

/// Canonicalizes a colour token to the `#rrggbb` form GtkSourceView schemes take.
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

#[cfg(test)]
mod tests;
