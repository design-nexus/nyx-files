// SPDX-License-Identifier: MIT

//! The Icons view: a grid of thumbnails with names under them, at the size
//! the folder (or the default) asks for.

use std::{cell::Cell, rc::Rc};

use gtk::{glib, prelude::*};

use super::model::{self, EntryModel, Hooks};

pub(super) struct IconsView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::GridView,
    hooks: Rc<Hooks>,
    size: Rc<Cell<i32>>,
}

/// Tile spacing and the narrowest tile, as the grid had them before.
const SPACING: i32 = 4;
const MIN_TILE: i32 = 116;
const ICON_PADDING: i32 = 6;

/// A tile is its thumbnail and padding, never narrower than `MIN_TILE`; names
/// wrap inside it rather than widen it.
fn tile_width(size: i32) -> i32 {
    (size + ICON_PADDING * 2).max(MIN_TILE)
}

fn factory(hooks: &Rc<Hooks>, size: i32) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let h = hooks.clone();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
        tile.add_css_class("icon-tile");
        tile.set_size_request(tile_width(size), -1);
        let icon = model::icon_slot(size);
        icon.add_css_class("tile-icon");
        icon.set_margin_top(ICON_PADDING);
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
        tile.append(&icon);
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
        if let Some(icon) = tile
            .first_child()
            .and_downcast::<crate::ui::thumbnail::ThumbnailSlot>()
        {
            model::show_icon(&icon, &r.entry, size);
        }
        let name = tile.first_child().and_then(|c| c.next_sibling());
        if let Some(label) = name
            .as_ref()
            .and_then(|c| c.downcast_ref::<gtk::Inscription>())
        {
            label.set_text(Some(&r.entry.display_name));
        }
        if let Some(detail) = tile.last_child().and_downcast::<gtk::Label>() {
            let text = super::list::size_text(&r.entry);
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
        self.root.add_tick_callback(move |root, _| {
            let width = root.width();
            if last.replace((width, size.get())) != (width, size.get())
                && width > 0
                && let Some(view) = view.upgrade()
            {
                // The grid's own 16 px of side padding is not tile space.
                let columns = ((width - 16) / (tile_width(size.get()) + SPACING)).max(1) as u32;
                if view.max_columns() != columns {
                    view.set_max_columns(columns);
                }
            }
            glib::ControlFlow::Continue
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
