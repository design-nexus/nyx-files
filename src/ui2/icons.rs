// SPDX-License-Identifier: MIT

//! The Icons view: a grid of large icons with names under them.

use gtk::prelude::*;

use super::{
    list::entry_icon,
    model::{self, EntryModel},
};

pub(super) struct IconsView {
    pub(super) root: gtk::ScrolledWindow,
    pub(super) view: gtk::GridView,
}

impl IconsView {
    pub(super) fn new(model: &EntryModel) -> Self {
        let factory = gtk::SignalListItemFactory::new();
        let hooks = model.hooks.clone();
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
            tile.add_css_class("icon-tile");
            tile.set_size_request(104, -1);
            let icon = gtk::Image::new();
            icon.set_pixel_size(64);
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
            model::on_context(&tile, &hooks, move || weak.upgrade().map(|i| i.position()));
        model::dnd_on(&tile, item, &hooks);
            item.set_child(Some(&tile));
        });
        let hooks = model.hooks.clone();
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let (Some(tile), Some(object)) = (item.child(), item.item()) else { return };
            let Some(r) = model::row_of(&object) else { return };
            hooks.bound(&r);
            if let Some(icon) = tile.first_child().and_downcast::<gtk::Image>() {
                icon.set_from_gicon(&entry_icon(&r.entry));
            }
            if let Some(label) = tile.last_child().and_downcast::<gtk::Label>() {
                label.set_text(&r.entry.display_name);
            }
            tile.set_tooltip_text(Some(&r.entry.display_name));
            tile.set_opacity(if r.entry.is_hidden { 0.6 } else { 1.0 });
        });
        let view = gtk::GridView::new(Some(model.selection.clone()), Some(factory));
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
        Self { root, view }
    }
}
