// SPDX-License-Identifier: MIT

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    ffi::CString,
    fs, io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use gtk::{gdk, gio, glib, prelude::*};

pub mod icons {
    //! Themed symbolic icon names (Umbra in the suite, any freedesktop theme
    //! elsewhere) for the interface's glyphs.

    pub const BLOCKS: &str = "application-x-addon-symbolic";
    pub const BUG: &str = "applications-science-symbolic";
    pub const CHECK: &str = "object-select-symbolic";
    pub const CIRCLE_CHECK: &str = "object-select-symbolic";
    pub const CIRCLE_X: &str = "dialog-error-symbolic";
    pub const CLOUD: &str = "weather-overcast-symbolic";
    pub const CODE_XML: &str = "text-html-symbolic";
    pub const COPY: &str = "edit-copy-symbolic";
    pub const DOCUMENTS: &str = "text-x-generic-symbolic";
    pub const DOWNLOADS: &str = "folder-download-symbolic";
    pub const EJECT: &str = "media-eject-symbolic";
    pub const EYE: &str = "view-reveal-symbolic";
    pub const EYE_OFF: &str = "view-conceal-symbolic";
    pub const FILE_ARCHIVE: &str = "package-x-generic-symbolic";
    pub const FILE_CODE: &str = "text-x-script-symbolic";
    pub const FOLDER: &str = "folder-symbolic";
    pub const GIT_BRANCH: &str = "folder-remote-symbolic";
    pub const GLOBE: &str = "web-browser-symbolic";
    pub const HARD_DRIVE: &str = "drive-harddisk-symbolic";
    pub const HOME: &str = "user-home-symbolic";
    pub const KEY: &str = "dialog-password-symbolic";
    pub const LIBRARY: &str = "view-grid-symbolic";
    pub const LIST_CHECKS: &str = "checkbox-checked-symbolic";
    pub const LOCK: &str = "system-lock-screen-symbolic";
    pub const MINUS: &str = "list-remove-symbolic";
    pub const MONITOR: &str = "computer-symbolic";
    pub const NETWORK: &str = "network-workgroup-symbolic";
    pub const PENCIL: &str = "document-edit-symbolic";
    pub const PICTURES: &str = "image-x-generic-symbolic";
    pub const PIN: &str = "view-pin-symbolic";
    pub const PLAY: &str = "media-playback-start-symbolic";
    pub const PLUS: &str = "list-add-symbolic";
    pub const PRINTER: &str = "document-print-symbolic";
    pub const REFRESH: &str = "view-refresh-symbolic";
    pub const SCALE: &str = "applications-engineering-symbolic";
    pub const SCISSORS: &str = "edit-cut-symbolic";
    pub const SEARCH: &str = "system-search-symbolic";
    pub const SETTINGS: &str = "emblem-system-symbolic";
    pub const SMARTPHONE: &str = "phone-symbolic";
    pub const TERMINAL: &str = "utilities-terminal-symbolic";
    pub const TRASH: &str = "user-trash-symbolic";
    pub const TRIANGLE_ALERT: &str = "dialog-warning-symbolic";
    pub const VIDEOS: &str = "video-x-generic-symbolic";
    pub const X: &str = "window-close-symbolic";

    pub const CUSTOMIZATION_CHOICES: [(&str, &str); 16] = [
        (DOCUMENTS, "Documents"),
        (DOWNLOADS, "Downloads"),
        (FILE_CODE, "Code"),
        (FILE_ARCHIVE, "Archive"),
        (PICTURES, "Pictures"),
        (VIDEOS, "Videos"),
        (TERMINAL, "Terminal"),
        (HOME, "Home"),
        (HARD_DRIVE, "Storage"),
        (NETWORK, "Network"),
        (MONITOR, "Computer"),
        (KEY, "Private"),
        (PIN, "Pinned"),
        (PLAY, "Media"),
        (SETTINGS, "Settings"),
        (LIST_CHECKS, "Tasks"),
    ];

    /// The icon a choice saved under its old bundled name now uses.
    const SAVED_CHOICES: [(&str, &str); 16] = [
        ("strata-file-text", DOCUMENTS),
        ("strata-download", DOWNLOADS),
        ("strata-file-code", FILE_CODE),
        ("strata-file-archive", FILE_ARCHIVE),
        ("strata-image", PICTURES),
        ("strata-video", VIDEOS),
        ("strata-terminal", TERMINAL),
        ("strata-house", HOME),
        ("strata-hard-drive", HARD_DRIVE),
        ("strata-network", NETWORK),
        ("strata-monitor", MONITOR),
        ("strata-key", KEY),
        ("strata-pin", PIN),
        ("strata-play", PLAY),
        ("strata-settings", SETTINGS),
        ("strata-list-checks", LIST_CHECKS),
    ];

    /// A saved custom icon in today's names: choices saved before the 2.0
    /// interface used the bundled icons' names.
    pub fn current_choice(name: &str) -> &str {
        SAVED_CHOICES
            .iter()
            .find(|(saved, _)| *saved == name)
            .map_or(name, |(_, current)| current)
    }

    pub fn custom_emoji(name: &str) -> Option<&str> {
        let emoji = name.strip_prefix("emoji:")?;
        (!emoji.is_empty() && emoji.len() <= 64 && !emoji.chars().any(char::is_control))
            .then_some(emoji)
    }

    pub fn is_customization_choice(name: &str) -> bool {
        let name = current_choice(name);
        CUSTOMIZATION_CHOICES
            .iter()
            .any(|(icon_name, _)| *icon_name == name)
            || custom_emoji(name).is_some()
    }
}

const FONT_VERSION: &str = "2.304";
const ICON_TEXTURE_PX: i32 = 96;
const ICON_TEXTURE_CACHE_LIMIT: usize = 256;
const JETBRAINS_MONO: &[u8] = include_bytes!("../data/fonts/JetBrainsMono[wght].ttf");

thread_local! {
    static PRIMARY_ICON_COLOR: RefCell<String> = RefCell::new("#8bc9eb".to_owned());
    static MUTED_ICON_COLOR: RefCell<String> = RefCell::new("#9097a8".to_owned());
    static ICON_TEXTURES: RefCell<IconTextureCache> = RefCell::new(IconTextureCache::default());
}

pub fn prepare() -> Result<(), Box<dyn std::error::Error>> {
    let _startup = crate::util::ui_thread::allow(crate::util::ui_thread::Exception::Startup);
    gio::resources_register_include!("files.gresource")?;

    let font_directory = glib::user_cache_dir()
        .join("nyx-files")
        .join("fonts")
        .join(FONT_VERSION);
    fs::create_dir_all(&font_directory)?;

    let regular = font_directory.join("JetBrainsMono.ttf");
    write_if_changed(&regular, JETBRAINS_MONO)?;
    register_application_fonts([regular])?;

    Ok(())
}

pub fn register_icon_theme() {
    // Desktop shells resolve the window icon by matching the application ID to a
    // desktop entry, but GTK also needs the name to expose the bundled icon on its
    // own surfaces and on compositors that accept a toplevel icon.
    gtk::Window::set_default_icon_name(crate::APPLICATION_ID);
}

/// Toolbar and row action glyphs.
pub const CHROME_ICON_PX: i32 = 16;

/// A themed icon at `pixel_size`, in its widget's text colour.
pub fn primary_icon(name: &str, pixel_size: i32) -> gtk::Image {
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(pixel_size);
    image
}

pub fn text_icon(name: &str, pixel_size: i32) -> gtk::Image {
    primary_icon(name, pixel_size)
}

/// An icon in the danger colour.
pub fn danger_icon(name: &str, pixel_size: i32) -> gtk::Image {
    let image = primary_icon(name, pixel_size);
    image.add_css_class("danger-icon");
    image
}

pub fn set_primary_icon(image: &gtk::Image, name: &str) {
    image.set_icon_name(Some(name));
}

pub fn set_primary_icon_color(color: &str) {
    PRIMARY_ICON_COLOR.with(|current| current.replace(color.to_owned()));
}

pub fn set_muted_icon_color(color: &str) {
    MUTED_ICON_COLOR.with(|current| current.replace(color.to_owned()));
}

pub fn emoji_icon_paintable(emoji: &str) -> Option<gdk::Texture> {
    emoji_texture(emoji)
}

pub fn primary_icon_color() -> String {
    PRIMARY_ICON_COLOR.with(|color| color.borrow().clone())
}

/// A theme icon as a texture of `logical_px` at `scale`: full colour, or a
/// symbolic one drawn in `color`. `None` when the theme has no such icon.
pub(crate) fn theme_texture(
    name: &str,
    logical_px: i32,
    scale: i32,
    color: Option<&str>,
) -> Option<gdk::Texture> {
    let key = (format!("theme:{name}"), color.unwrap_or_default().to_owned(), logical_px * scale.max(1));
    if let Some(texture) = cached_icon_texture(&key) {
        return Some(texture);
    }
    let display = gdk::Display::default()?;
    let theme = gtk::IconTheme::for_display(&display);
    if !theme.has_icon(name) {
        return None;
    }
    let icon = theme.lookup_icon(
        name,
        &[],
        logical_px,
        scale.max(1),
        gtk::TextDirection::None,
        gtk::IconLookupFlags::empty(),
    );
    let path = icon.file()?.path()?;
    let px = logical_px * scale.max(1);
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_size(path, px, px).ok()?;
    let pixbuf = match color.and_then(|color| gdk::RGBA::parse(color).ok()) {
        Some(rgba) => recolored(&pixbuf, &rgba)?,
        None => pixbuf,
    };
    Some(cache_icon_texture(key, gdk::Texture::for_pixbuf(&pixbuf)))
}

/// A one-colour symbolic icon in `color`, keeping its shape's alpha.
fn recolored(pixbuf: &gdk_pixbuf::Pixbuf, color: &gdk::RGBA) -> Option<gdk_pixbuf::Pixbuf> {
    let pixbuf = if pixbuf.has_alpha() {
        pixbuf.copy()?
    } else {
        pixbuf.add_alpha(false, 0, 0, 0).ok()?
    };
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (red, green, blue) = (channel(color.red()), channel(color.green()), channel(color.blue()));
    let (width, height) = (pixbuf.width() as usize, pixbuf.height() as usize);
    let stride = pixbuf.rowstride() as usize;
    let channels = pixbuf.n_channels() as usize;
    // SAFETY: `pixbuf` is a fresh copy no one else holds, and every index stays
    // inside its `height` rows of `stride` bytes.
    #[expect(unsafe_code, reason = "GdkPixbuf exposes mutable pixels only through this call")]
    let pixels = unsafe { pixbuf.pixels() };
    for row in 0..height {
        for column in 0..width {
            let i = row * stride + column * channels;
            pixels[i] = red;
            pixels[i + 1] = green;
            pixels[i + 2] = blue;
        }
    }
    Some(pixbuf)
}

fn emoji_texture(emoji: &str) -> Option<gdk::Texture> {
    let key = (
        format!("emoji:{emoji}"),
        "native".to_owned(),
        ICON_TEXTURE_PX,
    );
    if let Some(texture) = cached_icon_texture(&key) {
        return Some(texture);
    }
    render_emoji_texture(key, emoji, 78.0, (82.0, 82.0), (48.0, 48.0), None)
}

fn render_emoji_texture(
    key: (String, String, i32),
    emoji: &str,
    preferred_size: f64,
    bounds: (f64, f64),
    center: (f64, f64),
    background: Option<&cairo::ImageSurface>,
) -> Option<gdk::Texture> {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, key.2, key.2).ok()?;
    let context = cairo::Context::new(&surface).ok()?;
    if let Some(background) = background {
        context.set_source_surface(background, 0.0, 0.0).ok()?;
        context.paint().ok()?;
    }
    let scale = f64::from(key.2) / f64::from(ICON_TEXTURE_PX);
    context.scale(scale, scale);

    let (layout, ink) = fitted_emoji_layout(&context, emoji, preferred_size, bounds.0, bounds.1);
    context.set_source_rgb(1.0, 1.0, 1.0);
    context.move_to(
        center.0 - f64::from(ink.x() + ink.width() / 2),
        center.1 - f64::from(ink.y() + ink.height() / 2),
    );
    pangocairo::functions::show_layout(&context, &layout);

    Some(cache_icon_texture(key, texture_from_surface(&surface)?))
}

fn fitted_emoji_layout(
    context: &cairo::Context,
    emoji: &str,
    preferred_size: f64,
    max_width: f64,
    max_height: f64,
) -> (gtk::pango::Layout, gtk::pango::Rectangle) {
    let layout = pangocairo::functions::create_layout(context);
    let mut font = gtk::pango::FontDescription::from_string("emoji");
    font.set_absolute_size(preferred_size * f64::from(gtk::pango::SCALE));
    layout.set_font_description(Some(&font));
    layout.set_text(emoji);

    let (mut ink, _) = layout.pixel_extents();
    let width = f64::from(ink.width().max(1));
    let height = f64::from(ink.height().max(1));
    let scale = (max_width / width).min(max_height / height).min(1.0);
    if scale < 1.0 {
        font.set_absolute_size(preferred_size * scale * f64::from(gtk::pango::SCALE));
        layout.set_font_description(Some(&font));
        ink = layout.pixel_extents().0;
    }
    (layout, ink)
}

fn texture_from_surface(surface: &cairo::ImageSurface) -> Option<gdk::Texture> {
    let mut bytes = None;
    surface
        .with_data(|data| bytes = Some(glib::Bytes::from_owned(data.to_vec())))
        .ok()?;
    let format = if cfg!(target_endian = "little") {
        gdk::MemoryFormat::B8g8r8a8Premultiplied
    } else {
        gdk::MemoryFormat::A8r8g8b8Premultiplied
    };
    Some(
        gdk::MemoryTexture::new(
            surface.width(),
            surface.height(),
            format,
            &bytes?,
            surface.stride() as usize,
        )
        .upcast(),
    )
}

type IconTextureKey = (String, String, i32);

struct CachedIconTexture {
    texture: gdk::Texture,
    generation: u64,
}

#[derive(Default)]
struct IconTextureCache {
    entries: HashMap<IconTextureKey, CachedIconTexture>,
    recent: VecDeque<(IconTextureKey, u64)>,
    generation: u64,
}

impl IconTextureCache {
    fn get(&mut self, key: &IconTextureKey) -> Option<gdk::Texture> {
        let cached = self.entries.get_mut(key)?;
        self.generation = self.generation.saturating_add(1);
        cached.generation = self.generation;
        self.recent.push_back((key.clone(), self.generation));
        let texture = cached.texture.clone();
        self.compact_recent();
        Some(texture)
    }

    fn insert(&mut self, key: IconTextureKey, texture: gdk::Texture) -> gdk::Texture {
        self.generation = self.generation.saturating_add(1);
        let generation = self.generation;
        self.entries.insert(
            key.clone(),
            CachedIconTexture {
                texture: texture.clone(),
                generation,
            },
        );
        self.recent.push_back((key, generation));
        while self.entries.len() > ICON_TEXTURE_CACHE_LIMIT {
            let Some((oldest_key, oldest_generation)) = self.recent.pop_front() else {
                break;
            };
            if self
                .entries
                .get(&oldest_key)
                .is_some_and(|cached| cached.generation == oldest_generation)
            {
                self.entries.remove(&oldest_key);
            }
        }
        self.compact_recent();
        texture
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.entries.clear();
        self.recent.clear();
        self.generation = 0;
    }

    fn compact_recent(&mut self) {
        if self.recent.len() > ICON_TEXTURE_CACHE_LIMIT * 4 {
            self.recent.retain(|(key, generation)| {
                self.entries
                    .get(key)
                    .is_some_and(|cached| cached.generation == *generation)
            });
        }
    }
}

/// Drops cached theme icons; a new or regenerated icon theme draws them again.
pub(crate) fn forget_theme_textures() {
    ICON_TEXTURES.with(|textures| *textures.borrow_mut() = IconTextureCache::default());
}

fn cached_icon_texture(key: &IconTextureKey) -> Option<gdk::Texture> {
    ICON_TEXTURES.with(|textures| textures.borrow_mut().get(key))
}

fn cache_icon_texture(key: IconTextureKey, texture: gdk::Texture) -> gdk::Texture {
    ICON_TEXTURES.with(|textures| textures.borrow_mut().insert(key, texture))
}

fn write_if_changed(path: &Path, contents: &[u8]) -> io::Result<()> {
    let is_current = fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file() && metadata.len() == contents.len() as u64)
        .unwrap_or(false);

    if !is_current {
        crate::storage::atomic_write(path, contents)?;
    }

    Ok(())
}

#[expect(
    unsafe_code,
    reason = "Fontconfig exposes application-font registration only through its C FFI"
)]
fn register_application_fonts(
    paths: impl IntoIterator<Item = PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: This no-argument Fontconfig call returns a borrowed process-global
    // configuration. We check it for null before passing it to any other FFI call.
    let config = unsafe { fontconfig_sys::FcConfigGetCurrent() };
    if config.is_null() {
        return Err("Fontconfig did not provide a current configuration".into());
    }

    for path in paths {
        let path = CString::new(path.as_os_str().as_bytes())?;

        // SAFETY: `config` was checked above. `path` is a valid, NUL-terminated C string
        // that remains alive for the call, and Fontconfig copies rather than retains it.
        let registered =
            unsafe { fontconfig_sys::FcConfigAppFontAddFile(config, path.as_ptr().cast()) };
        if registered == 0 {
            return Err("Fontconfig could not register a bundled font".into());
        }
    }

    // SAFETY: `config` is the same checked process-global configuration. Registration
    // runs during single-threaded startup before GTK/Pango creates the application's map.
    let rebuilt = unsafe { fontconfig_sys::FcConfigBuildFonts(config) };
    if rebuilt == 0 {
        return Err("Fontconfig could not rebuild the application font set".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests;
