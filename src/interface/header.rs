// SPDX-License-Identifier: MIT

//! The header's button groups: navigation, the path with its copy button, the
//! folder's actions (sort, thumbnail size, the filter field), and
//! the search field. Groups sit on a raised fill so they read as buttons.

use std::rc::Rc;

use gtk::{glib, prelude::*};
use nyx_ui::widgets;

use crate::{
    model::{SortDirection, SortKey},
    ui::{
        browser_modes::BrowserMode,
        keys::{Binding, Keys, bindings},
    },
};

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

fn button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = widgets::bar_button(icon, tooltip);
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

fn menu_button(icon: &str, tooltip: &str) -> (gtk::MenuButton, gtk::Popover) {
    let popover = gtk::Popover::new();
    popover.set_has_arrow(false);
    popover.add_css_class("menu-popover");
    let menu = gtk::MenuButton::new();
    menu.set_icon_name(icon);
    menu.add_css_class("bar-button");
    menu.set_tooltip_text(Some(tooltip));
    menu.set_popover(Some(&popover));
    menu.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    (menu, popover)
}

pub(super) struct Actions {
    pub(super) root: gtk::Box,
    leftovers: gtk::Button,
    empty_trash: gtk::Button,
    thumbnails: gtk::MenuButton,
    sort_direction: gtk::Button,
    sort: gtk::MenuButton,
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
        let (thumbnails, thumbnails_popover) = menu_button(
            "image-x-generic-symbolic",
            &crate::i18n::tr("Thumbnail size"),
        );
        let sort_direction = button("view-sort-ascending-symbolic", "");
        let (sort, sort_popover) = menu_button(
            "view-list-bullet-symbolic",
            &crate::i18n::tr("Choose sort field"),
        );
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
            thumbnails.upcast_ref(),
            sort_direction.upcast_ref(),
            sort.upcast_ref(),
            filter_revealer.upcast_ref(),
        ] {
            root.append(widget);
        }
        crate::ui::tenxer_mode::hide_while_enabled(&root);
        let actions = Rc::new(Actions {
            root,
            leftovers,
            empty_trash,
            thumbnails,
            sort_direction,
            sort,
            filter_toggle,
            filter_revealer,
        });
        actions.wire(window, filter, &thumbnails_popover, &sort_popover);
        actions
    }

    fn wire(
        &self,
        window: &Rc<Window>,
        filter: &gtk::SearchEntry,
        thumbnails: &gtk::Popover,
        sort: &gtk::Popover,
    ) {
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
        let (c, w) = (current.clone(), Rc::downgrade(window));
        self.sort_direction.connect_clicked(move |_| {
            let Some(tab) = c() else { return };
            let Some(depth) = tab.browser.active_depth() else {
                return;
            };
            let Some(p) = tab.browser.column_preferences(depth) else {
                return;
            };
            let direction = match p.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
            tab.browser.set_sort(depth, p.sort_key, direction);
            if let Some(w) = w.upgrade() {
                w.sync_header();
            }
        });
        let c = current.clone();
        sort.connect_show(move |p| p.set_child(Some(&super::appearance::sort_menu(&c))));
        let c = current.clone();
        thumbnails.connect_show(move |p| p.set_child(Some(&super::appearance::thumbnail_menu(&c))));
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
        let mode = tab.mode();
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
        self.thumbnails.set_visible(mode == BrowserMode::Icons);
        let sorts = mode != BrowserMode::List;
        self.sort.set_visible(sorts);
        self.sort_direction.set_visible(sorts);
        let prefs = tab
            .browser
            .active_depth()
            .and_then(|d| tab.browser.column_preferences(d));
        let device_order = prefs.is_some_and(|p| p.sort_key == SortKey::DeviceOrder);
        let descending = prefs.is_some_and(|p| p.sort_direction == SortDirection::Descending);
        self.sort_direction.set_sensitive(!device_order);
        self.sort_direction.set_icon_name(if descending {
            "view-sort-descending-symbolic"
        } else {
            "view-sort-ascending-symbolic"
        });
        let hint = crate::i18n::tr(if device_order {
            "Device order follows discovery; choose a sort field to reverse its direction"
        } else if descending {
            "Descending — click to reverse"
        } else {
            "Ascending — click to reverse"
        });
        self.sort_direction.set_tooltip_text(Some(&hint));
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
/// them, or Ctrl+L, to type a path) and a button that copies the folder's path.
pub(super) fn path_group(path: &nyx_ui::pathbar::PathBar, window: &Rc<Window>) -> gtk::Box {
    let root = group();
    root.add_css_class("path-group");
    wire_path_edit(path);
    root.append(&path.root);
    path.root.set_hexpand(true);
    let copy = button("edit-copy-symbolic", &crate::i18n::tr("Copy path"));
    let w = Rc::downgrade(window);
    copy.connect_clicked(move |button| {
        let Some(location) = w
            .upgrade()
            .and_then(|w| w.current())
            .and_then(|t| t.location())
        else {
            return;
        };
        let text = location
            .native_path()
            .map_or_else(|| location.display_path(), |p| p.display().to_string());
        button.clipboard().set_text(&text);
        button.set_tooltip_text(Some(&crate::i18n::tr("Path copied")));
        let b = button.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
            if let Some(b) = b.upgrade() {
                b.set_tooltip_text(Some(&crate::i18n::tr("Copy path")));
            }
        });
    });
    root.append(&copy);
    root
}

/// The search field: opens search across Home and the drives (Ctrl+K).
pub(super) fn search_field(window: &Rc<Window>) -> gtk::Button {
    let field = gtk::Button::new();
    field.add_css_class("header-search-field");
    field.set_valign(gtk::Align::Center);
    let tooltip = with_keys("Search files and folders (%{keys})", bindings::SEARCH);
    field.set_tooltip_text(Some(&tooltip));
    field.update_property(&[gtk::accessible::Property::Label(&tooltip)]);
    let row = widgets::hbox(8);
    row.append(&gtk::Image::from_icon_name("system-search-symbolic"));
    let label = widgets::label(&crate::i18n::tr("Search"), "dim");
    label.set_hexpand(true);
    label.set_xalign(0.0);
    row.append(&label);
    field.set_child(Some(&row));
    let w = Rc::downgrade(window);
    field.connect_clicked(move |_| {
        if let Some(w) = w.upgrade() {
            super::search::open(&w, super::search::Mode::Everywhere);
        }
    });
    field
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
