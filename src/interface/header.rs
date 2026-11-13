// SPDX-License-Identifier: MIT

//! The header's parts: the path, the folder's actions (leftovers, Empty Trash,
//! the filter field) and the New button. Sort and thumbnail size are in the
//! View menu, and Icons has them over its grid.

use std::rc::Rc;

use gtk::prelude::*;
use nyx_ui::widgets;

use crate::ui::keys::{Binding, Keys, bindings};

use super::{tab::Tab, window::Window};

fn with_keys(message: &str, binding: Binding) -> String {
    crate::ui::shortcut_reference::with_keys(message, Keys::One(binding))
}

/// A row of buttons on one raised fill.
pub(super) fn group() -> gtk::Box {
    let group = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    group.add_css_class("header-group");
    group.set_valign(gtk::Align::Center);
    group
}

/// Buttons joined into one linked block, as View and Settings at the header's end.
pub(super) fn linked(buttons: &[&gtk::Widget]) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    block.add_css_class("linked");
    block.add_css_class("header-linked");
    block.set_valign(gtk::Align::Center);
    for (i, button) in buttons.iter().enumerate() {
        if i > 0 {
            let split = gtk::Separator::new(gtk::Orientation::Vertical);
            split.add_css_class("linked-split");
            buttons[i - 1]
                .bind_property("visible", &split, "visible")
                .sync_create()
                .build();
            block.append(&split);
        }
        button.set_valign(gtk::Align::Fill);
        block.append(*button);
    }
    block
}

fn button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = widgets::bar_button(icon, tooltip);
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

pub(super) struct Actions {
    pub(super) root: gtk::Box,
    leftovers: gtk::Button,
    empty_trash: gtk::Button,
    pub(super) filter_toggle: gtk::ToggleButton,
    pub(super) filter_revealer: gtk::Revealer,
}

impl Actions {
    pub(super) fn new(window: &Rc<Window>, filter: &gtk::SearchEntry) -> Rc<Self> {
        let root = group();
        let leftovers = button(
            "dialog-warning-symbolic",
            &crate::i18n::tr("Items left by an interrupted operation"),
        );
        leftovers.add_css_class("leftovers-button");
        let empty_trash = button("user-trash-symbolic", &crate::i18n::tr("Empty Trash"));
        let filter_toggle = gtk::ToggleButton::new();
        filter_toggle.set_icon_name("view-filter-symbolic");
        filter_toggle.add_css_class("bar-button");
        let filter_label = with_keys("Filter this folder (%{keys})", bindings::FILTER);
        filter_toggle.set_tooltip_text(Some(&filter_label));
        filter_toggle.update_property(&[gtk::accessible::Property::Label(&filter_label)]);
        let filter_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .child(filter)
            .build();
        // The filter toggle stays off the bar; Ctrl+F, / or typing opens the field.
        for widget in [
            leftovers.upcast_ref::<gtk::Widget>(),
            empty_trash.upcast_ref(),
            filter_revealer.upcast_ref(),
        ] {
            root.append(widget);
        }
        crate::ui::tenxer_mode::hide_while_enabled(&root);
        let actions = Rc::new(Actions {
            root,
            leftovers,
            empty_trash,
            filter_toggle,
            filter_revealer,
        });
        actions.wire(window, filter);
        actions
    }

    fn wire(&self, window: &Rc<Window>, filter: &gtk::SearchEntry) {
        let current = {
            let weak = Rc::downgrade(window);
            Rc::new(move || weak.upgrade().and_then(|w| w.current()))
                as Rc<dyn Fn() -> Option<Rc<Tab>>>
        };
        let c = current.clone();
        self.leftovers.connect_clicked(move |_| {
            if let Some(tab) = c()
                && let Some(depth) = tab.browser.active_depth()
            {
                tab.browser.request_leftover_review(depth);
            }
        });
        let w = Rc::downgrade(window);
        self.empty_trash.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                super::actions::empty_trash(&w);
            }
        });
        let (revealer, entry) = (self.filter_revealer.clone(), filter.clone());
        let c = current.clone();
        self.filter_toggle.connect_toggled(move |toggle| {
            revealer.set_reveal_child(toggle.is_active());
            if toggle.is_active() {
                entry.grab_focus();
            } else {
                entry.set_text("");
                if let Some(tab) = c() {
                    tab.set_filter("");
                    tab.focus_view();
                }
            }
        });
    }

    /// Shows the actions that apply to the folder showing and its view.
    pub(super) fn sync(&self, tab: &Tab) {
        let location = tab.location();
        let in_trash = location
            .as_ref()
            .is_some_and(|l| l.uri_value() == Some("trash:///"));
        self.empty_trash.set_visible(in_trash);
        let leftovers = tab
            .browser
            .active_depth()
            .is_some_and(|d| !tab.browser.column_leftovers(d).is_empty());
        self.leftovers.set_visible(leftovers);
        if tab.filtering() && !self.filter_toggle.is_active() {
            self.filter_toggle.set_active(true);
        }
    }

    /// Opens the filter field (Ctrl+F, /).
    pub(super) fn open_filter(&self, filter: &gtk::SearchEntry) {
        if self.filter_toggle.is_active() {
            filter.grab_focus();
        } else {
            self.filter_toggle.set_active(true);
        }
    }
}

/// The gap after the crumbs opens the path for typing, so a folder with no
/// arrows between crumbs, such as `/`, can still be typed over.
pub(super) fn wire_path_edit(path: &nyx_ui::pathbar::PathBar) {
    let mut stack = vec![path.root.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = stack.pop() {
        let mut child = widget.first_child();
        while let Some(c) = child {
            stack.push(c.clone());
            child = c.next_sibling();
        }
        if widget.has_css_class("pathbar-filler") {
            widget.set_cursor_from_name(Some("text"));
            widget.set_tooltip_text(Some(&crate::i18n::tr("Type a path")));
        }
    }
    let entry = path.entry.clone();
    path.root.connect_visible_child_name_notify(move |root| {
        if root.visible_child_name().as_deref() != Some("entry") {
            entry.set_size_request(-1, -1);
            return;
        }
        let (text, _) = entry.create_pango_layout(Some(&entry.text())).pixel_size();
        let width = root.width().max(text + 32).min(path_room(root));
        entry.set_size_request(width, -1);
    });
}

/// How wide the path can grow before it would push into the header's end.
fn path_room(root: &gtk::Stack) -> i32 {
    let Some(header) = root
        .ancestor(gtk::CenterBox::static_type())
        .and_downcast::<gtk::CenterBox>()
    else {
        return root.width();
    };
    let x = |widget: &gtk::Widget| {
        root.compute_point(widget, &gtk::graphene::Point::zero())
            .map_or(0, |p| p.x() as i32)
    };
    let trailing = root
        .ancestor(nyx_ui::clamp::Clamp::static_type())
        .map_or(0, |clamp| clamp.width() - x(&clamp) - root.width());
    let end = header
        .end_widget()
        .map_or(0, |w| w.measure(gtk::Orientation::Horizontal, -1).0);
    (header.width() - end - x(header.upcast_ref()) - trailing - 12).max(root.width())
}

/// The path group: breadcrumbs (click the gap after them, an arrow between
/// them, or Ctrl+L, to type a path). Copy path is in the folder's menu.
pub(super) fn path_group(path: &nyx_ui::pathbar::PathBar) -> gtk::Box {
    let root = group();
    root.add_css_class("path-group");
    wire_path_edit(path);
    root.append(&path.root);
    path.root.set_hexpand(true);
    root
}

/// The header's New button; its menu makes folders, files, tabs and windows.
pub(super) fn new_button() -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("new-button");
    button.set_valign(gtk::Align::Center);
    let row = widgets::hbox(6);
    row.set_valign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name("folder-new-symbolic");
    icon.add_css_class("new-icon");
    let label = gtk::Label::new(Some(&crate::i18n::tr("New")));
    label.add_css_class("new-label");
    let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
    arrow.add_css_class("new-arrow");
    for part in [
        icon.upcast_ref::<gtk::Widget>(),
        label.upcast_ref(),
        arrow.upcast_ref(),
    ] {
        part.set_valign(gtk::Align::Center);
        row.append(part);
    }
    button.set_child(Some(&row));
    let tooltip = crate::i18n::tr("New folder, file, tab or window");
    button.set_tooltip_text(Some(&tooltip));
    button.update_property(&[gtk::accessible::Property::Label(&tooltip)]);
    button
}

/// The app's name beside its icon at the start of the header.
pub(super) fn app_title(header: &impl IsA<gtk::Widget>) -> Option<gtk::Widget> {
    let mut stack = vec![header.as_ref().clone()];
    while let Some(widget) = stack.pop() {
        if widget.has_css_class("app-title") {
            return Some(widget);
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            stack.push(c.clone());
            child = c.next_sibling();
        }
    }
    None
}

/// In a narrow window New shows only its icon.
pub(super) fn set_new_narrow(button: &gtk::Button, narrow: bool) {
    let mut child = button.child().and_then(|row| row.first_child());
    while let Some(widget) = child {
        if widget.has_css_class("new-label") || widget.has_css_class("new-arrow") {
            widget.set_visible(!narrow);
        }
        child = widget.next_sibling();
    }
}

/// Clicking an arrow between the crumbs turns the path into a field to type
/// in, as clicking the path's empty end does.
pub(super) fn wire_separators(path: &Rc<nyx_ui::pathbar::PathBar>, text: &str) {
    let mut stack = vec![path.root.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = stack.pop() {
        let mut child = widget.first_child();
        while let Some(c) = child {
            stack.push(c.clone());
            child = c.next_sibling();
        }
        if !widget.has_css_class("crumb-sep") || widget.has_css_class("edit-wired") {
            continue;
        }
        widget.add_css_class("edit-wired");
        widget.set_cursor_from_name(Some("text"));
        widget.set_tooltip_text(Some(&crate::i18n::tr("Type a path")));
        let click = gtk::GestureClick::new();
        let (bar, text) = (Rc::downgrade(path), text.to_string());
        click.connect_released(move |_, _, _, _| {
            if let Some(bar) = bar.upgrade() {
                bar.edit(&text);
            }
        });
        widget.add_controller(click);
    }
}
