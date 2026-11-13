// SPDX-License-Identifier: MIT

//! The Icons view: a grid of thumbnails with names under them, at the size
//! the folder (or the default) asks for. Photos fill a rounded 4:3 frame;
//! other items show their icon in it. Selected tiles are ringed and ticked.

use std::{cell::Cell, rc::Rc};

use gtk::prelude::*;

use super::model::{self, EntryModel, Hooks};

pub(super) struct IconsView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::GridView,
    hooks: Rc<Hooks>,
    size: Rc<Cell<i32>>,
}

/// Tile spacing and the narrowest tile.
const SPACING: i32 = 14;
const MIN_TILE: i32 = 96;
const ICON_PADDING: i32 = 4;

/// A frame `size` tall and a third wider.
fn frame_width(size: i32) -> i32 {
    size * 4 / 3
}

/// A tile is its frame and padding, never narrower than `MIN_TILE`; names
/// wrap inside it rather than widen it.
fn tile_width(size: i32) -> i32 {
    frame_width(size).max(MIN_TILE)
}

/// The icon an item without a thumbnail shows in its frame.
fn icon_size(size: i32) -> i32 {
    (size * 9 / 16).clamp(24, 128)
}

fn slot_of(tile: &gtk::Widget) -> Option<crate::ui::thumbnail::ThumbnailSlot> {
    tile.first_child()
        .and_downcast::<gtk::Overlay>()
        .and_then(|o| o.child())
        .and_downcast::<crate::ui::thumbnail::ThumbnailSlot>()
}

fn factory(hooks: &Rc<Hooks>, size: i32) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let h = hooks.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
        tile.add_css_class("icon-tile");
        tile.set_size_request(tile_width(size), -1);
        let icon = model::icon_slot(icon_size(size));
        icon.add_css_class("tile-icon");
        icon.set_margin_top(ICON_PADDING);
        icon.set_halign(gtk::Align::Center);
        let framed = gtk::Overlay::new();
        framed.set_halign(gtk::Align::Center);
        framed.set_child(Some(&icon));
        let tick = gtk::Image::from_icon_name("object-select-symbolic");
        tick.add_css_class("tile-check");
        tick.set_halign(gtk::Align::End);
        tick.set_valign(gtk::Align::Start);
        tick.set_margin_top(ICON_PADDING + 7);
        tick.set_margin_end(7);
        tick.set_can_target(false);
        framed.add_overlay(&tick);
        let label = gtk::Inscription::new(None);
        label.set_min_chars(0);
        label.set_nat_chars(0);
        label.set_min_lines(1);
        label.set_nat_lines(2);
        label.set_xalign(0.5);
        label.set_yalign(0.0);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_text_overflow(gtk::InscriptionOverflow::EllipsizeMiddle);
        label.add_css_class("tile-name");
        let detail = gtk::Label::new(None);
        detail.add_css_class("tile-detail");
        tile.append(&framed);
        tile.append(&label);
        tile.append(&detail);
        let weak = item.downgrade();
        model::on_context(&tile, &h, move || weak.upgrade().map(|i| i.position()));
        model::dnd_on(&tile, item, &h);
        item.set_child(Some(&tile));
    });
    let h = hooks.clone();
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(tile), Some(object)) = (item.child(), item.item()) else {
            return;
        };
        let Some(r) = model::row_of(&object) else {
            return;
        };
        h.bound(&r);
        if let Some(icon) = slot_of(&tile) {
            let (content_type, _) =
                gtk::gio::content_type_guess(Some(&r.entry.native_name), None::<&[u8]>);
            icon.set_frame(Some(crate::ui::thumbnail::SlotFrame {
                min_width: frame_width(size),
                width: frame_width(size),
                height: size,
                cover: content_type.starts_with("image/") || content_type.starts_with("video/"),
                radius: 8.0,
            }));
            let name = crate::ui::thumbnail::theme_icon(
                &crate::ui::file_icons::name_for(&r.entry),
                r.entry.is_directory(),
            );
            crate::ui::thumbnail::set_thumbnail_or_icon(
                &icon,
                &r.entry,
                &name,
                icon_size(size),
                (size * 2).min(512),
            );
            icon.set_hidden(r.entry.is_hidden);
        }
        let name = tile.first_child().and_then(|c| c.next_sibling());
        if let Some(label) = name
            .as_ref()
            .and_then(|c| c.downcast_ref::<gtk::Inscription>())
        {
            label.set_text(Some(&r.entry.display_name));
        }
        if let Some(detail) = tile.last_child().and_downcast::<gtk::Label>() {
            // Folders say how much is in them; a file's name is enough under its picture.
            let text = if r.entry.is_directory() {
                super::list::size_text(&r.entry)
            } else {
                String::new()
            };
            detail.set_visible(!text.is_empty());
            detail.set_text(&text);
        }
        tile.set_tooltip_text(Some(&r.entry.display_name));
        tile.set_opacity(if r.entry.is_hidden { 0.6 } else { 1.0 });
    });
    model::cancel_on_unbind(&factory);
    factory
}

impl IconsView {
    pub(super) fn new(model: &EntryModel) -> Self {
        let size = crate::ui::preferences::PreferenceManager::shared().icons_thumbnail_size();
        let view = gtk::GridView::new(
            Some(model.selection.clone()),
            Some(factory(&model.hooks, size)),
        );
        view.add_css_class("file-grid");
        view.set_max_columns(32);
        view.set_min_columns(1);
        view.set_enable_rubberband(true);
        model::on_context(&view, &model.hooks, || None);
        model::drop_on_view(&view, &model.hooks);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        let icons = Self {
            root,
            view,
            hooks: model.hooks.clone(),
            size: Rc::new(Cell::new(size)),
        };
        icons.pin_columns();
        icons
    }

    /// As many columns as whole tiles fit, so the grid does not spread a few
    /// columns across the width.
    fn pin_columns(&self) {
        let (view, size) = (self.view.downgrade(), self.size.clone());
        let last = Cell::new((-1, -1));
        crate::ui::frame::after_layout(&self.root, move |root| {
            let width = root.width();
            if last.replace((width, size.get())) != (width, size.get())
                && width > 0
                && let Some(view) = view.upgrade()
            {
                // The grid's own 32 px of side padding is not tile space.
                let columns = ((width - 32 + SPACING) / (tile_width(size.get()) + SPACING)).max(1) as u32;
                if view.max_columns() != columns {
                    view.set_max_columns(columns);
                }
            }
        });
    }

    pub(super) fn size(&self) -> i32 {
        self.size.get()
    }

    /// Redraws the tiles at `size` px.
    pub(super) fn set_size(&self, size: i32) {
        if self.size.replace(size) != size {
            self.view.set_factory(Some(&factory(&self.hooks, size)));
        }
    }
}

/// Over the Icons grid: the folder's name and count, the thumbnail size, and the sort.
pub(super) struct IconsBar {
    pub(super) root: gtk::Box,
    title: gtk::Label,
    count: gtk::Label,
    scale: gtk::Scale,
    sort: gtk::MenuButton,
    sort_label: gtk::Label,
}

impl IconsBar {
    pub(super) fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("icons-bar");
        let title = nyx_ui::widgets::label("", "icons-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        root.append(&title);
        let count = nyx_ui::widgets::label("", "icons-count");
        count.set_hexpand(true);
        root.append(&count);
        let size = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        size.add_css_class("icons-size");
        let small = gtk::Image::from_icon_name("image-x-generic-symbolic");
        small.set_pixel_size(12);
        let scale = gtk::Scale::with_range(
            gtk::Orientation::Horizontal,
            f64::from(crate::ui::icons_cell::MIN_ICONS_THUMBNAIL_SIZE),
            f64::from(crate::ui::icons_cell::MAX_ICONS_THUMBNAIL_SIZE),
            8.0,
        );
        scale.set_draw_value(false);
        scale.set_size_request(110, -1);
        scale.set_valign(gtk::Align::Center);
        let tip = crate::i18n::tr("Thumbnail size");
        scale.set_tooltip_text(Some(&tip));
        scale.update_property(&[gtk::accessible::Property::Label(&tip)]);
        let large = gtk::Image::from_icon_name("image-x-generic-symbolic");
        large.set_pixel_size(17);
        size.append(&small);
        size.append(&scale);
        size.append(&large);
        root.append(&size);
        let sort = gtk::MenuButton::new();
        sort.add_css_class("icons-sort");
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.append(&gtk::Image::from_icon_name("view-sort-ascending-symbolic"));
        let sort_label = gtk::Label::new(None);
        row.append(&sort_label);
        let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
        arrow.add_css_class("new-arrow");
        row.append(&arrow);
        sort.set_child(Some(&row));
        sort.set_tooltip_text(Some(&crate::i18n::tr("Choose sort field")));
        let popover = gtk::Popover::new();
        popover.set_has_arrow(false);
        popover.add_css_class("menu-popover");
        sort.set_popover(Some(&popover));
        root.append(&sort);
        Self {
            root,
            title,
            count,
            scale,
            sort,
            sort_label,
        }
    }

    /// Size changes go to `tab`, and the sort menu acts on it.
    pub(super) fn wire(&self, tab: std::rc::Weak<super::tab::Tab>) {
        let (t, guard) = (tab.clone(), self.root.clone());
        self.scale.connect_value_changed(move |scale| {
            if guard.has_css_class("syncing") {
                return;
            }
            if let Some(tab) = t.upgrade() {
                tab.set_icons_size(scale.value().round() as i32);
            }
        });
        let current: super::appearance::CurrentTab = Rc::new(move || tab.upgrade());
        if let Some(popover) = self.sort.popover() {
            popover.connect_show(move |p| p.set_child(Some(&super::appearance::sort_menu(&current))));
        }
    }

    pub(super) fn sync(&self, title: &str, count: &str, size: i32, sort: &str) {
        self.root.add_css_class("syncing");
        self.title.set_text(title);
        self.count.set_text(count);
        if self.scale.value().round() as i32 != size {
            self.scale.set_value(f64::from(size));
        }
        self.sort_label.set_text(sort);
        self.root.remove_css_class("syncing");
    }
}

/// Floats over the grid while items are selected: how many and how big, and
/// what to do with them, labelled.
pub(super) fn selection_bar(
    window: &Rc<super::window::Window>,
    tab: &Rc<super::tab::Tab>,
) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    bar.add_css_class("selection-bar");
    bar.set_halign(gtk::Align::Center);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_bottom(16);
    let count = nyx_ui::widgets::label("", "selection-count");
    bar.append(&count);
    let size = nyx_ui::widgets::label("", "selection-size");
    bar.append(&size);
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let action = |icon: &str, label: &str| {
        let button = nyx_ui::widgets::labeled_button(icon, &crate::i18n::tr(label));
        button.add_css_class("flat");
        button.add_css_class("selection-action");
        bar.append(&button);
        button
    };
    let copy = action("edit-copy-symbolic", "Copy");
    let move_to = action("go-jump-symbolic", "Move to…");
    let compress = action("package-x-generic-symbolic", "Compress");
    let send = action("document-send-symbolic", "Send to");
    let trash = action("user-trash-symbolic", "Move to Trash");
    trash.add_css_class("danger-label");
    bar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    let clear = nyx_ui::widgets::icon_button(
        "window-close-symbolic",
        &crate::i18n::tr("Clear selection (Esc)"),
    );
    bar.append(&clear);
    bar.set_visible(false);

    let t = Rc::downgrade(tab);
    copy.connect_clicked(move |_| {
        if let Some(tab) = t.upgrade() {
            super::actions::copy(&tab.selected(), false);
        }
    });
    let (w, t) = (Rc::downgrade(window), Rc::downgrade(tab));
    move_to.connect_clicked(move |_| {
        if let (Some(w), Some(tab)) = (w.upgrade(), t.upgrade()) {
            super::menus::transfer_to(&w, &tab, tab.selected(), true);
        }
    });
    let (w, t) = (Rc::downgrade(window), Rc::downgrade(tab));
    compress.connect_clicked(move |_| {
        if let (Some(w), Some(tab)) = (w.upgrade(), t.upgrade()) {
            super::operations::compress(&w, &tab, tab.selected());
        }
    });
    let (w, t) = (Rc::downgrade(window), Rc::downgrade(tab));
    send.connect_clicked(move |b| {
        if let (Some(w), Some(tab)) = (w.upgrade(), t.upgrade()) {
            let items = super::menus::send_to_items(&w, &tab, &tab.selected());
            nyx_ui::menu::popup_below(b, items);
        }
    });
    let (w, t) = (Rc::downgrade(window), Rc::downgrade(tab));
    trash.connect_clicked(move |_| {
        if let (Some(w), Some(tab)) = (w.upgrade(), t.upgrade()) {
            super::menus::remove(&w, &tab, tab.selected());
        }
    });
    let t = Rc::downgrade(tab);
    clear.connect_clicked(move |_| {
        if let Some(tab) = t.upgrade() {
            tab.model.selection.unselect_all();
            tab.focus_view();
        }
    });

    let (b, t) = (bar.downgrade(), Rc::downgrade(tab));
    let update = move || {
        let (Some(bar), Some(tab)) = (b.upgrade(), t.upgrade()) else {
            return;
        };
        let selected = tab.model.selected();
        let icons = tab.mode() == crate::ui::browser_modes::BrowserMode::Icons;
        bar.set_visible(icons && !selected.is_empty());
        if selected.is_empty() {
            return;
        }
        count.set_text(&rust_i18n::t!("%{count} selected", count = selected.len()));
        let bytes: u64 = selected
            .iter()
            .filter_map(|e| match e.size {
                crate::model::MetadataValue::Known(n) if !e.is_directory() => Some(n),
                _ => None,
            })
            .sum();
        size.set_text(&if bytes > 0 { gtk::glib::format_size(bytes).to_string() } else { String::new() });
        let native = selected.iter().all(|e| e.location.native_path().is_some());
        let trashed = tab.location().is_some_and(|l| l.uri_value() == Some("trash:///"));
        move_to.set_sensitive(!trashed);
        compress.set_sensitive(native && !trashed);
        send.set_visible(!crate::ui::window::devices::removable_destinations().is_empty());
        trash.set_sensitive(!trashed);
    };
    let u = update.clone();
    tab.model
        .selection
        .connect_selection_changed(move |_, _, _| u());
    let u = update.clone();
    tab.model.selection.connect_items_changed(move |_, _, _, _| u());
    tab.connect_changed(update);
    bar
}
