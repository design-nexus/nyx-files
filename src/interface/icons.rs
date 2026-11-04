// SPDX-License-Identifier: MIT

//! The Icons view: a grid of thumbnails with names under them, at the size
//! the folder (or the default) asks for.

use std::{cell::Cell, rc::Rc};

use gtk::prelude::*;

use super::model::{self, EntryModel, Hooks};

pub(super) struct IconsView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::GridView,
    hooks: Rc<Hooks>,
    size: Cell<i32>,
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
        tile.set_size_request((size + 40).max(104), -1);
        let icon = model::icon_slot(size);
        icon.add_css_class("tile-icon");
        let label = gtk::Label::new(None);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::Word);
        label.set_lines(2);
        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        label.set_justify(gtk::Justification::Center);
        label.set_max_width_chars(14);
        label.add_css_class("tile-name");
        tile.append(&icon);
        tile.append(&label);
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
        if let Some(label) = tile.last_child().and_downcast::<gtk::Label>() {
            label.set_text(&r.entry.display_name);
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
        view.set_min_columns(2);
        view.set_enable_rubberband(true);
        model::on_context(&view, &model.hooks, || None);
        model::drop_on_view(&view, &model.hooks);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();
        Self {
            root,
            view,
            hooks: model.hooks.clone(),
            size: Cell::new(size),
        }
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
