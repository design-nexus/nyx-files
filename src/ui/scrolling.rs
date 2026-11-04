// SPDX-License-Identifier: MIT

//! Scrolling shared by collection views: middle-click autoscroll, page-sized
//! keyboard navigation, and wheel routing for transient browser panels.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

pub(super) mod popover;







/// Starts autoscroll on `scroll` through the same `AutoScroll::start` path a
/// middle-click uses, so Escape can be tested without synthesizing a button event.
#[cfg(test)]
pub(super) fn begin_autoscroll_for_test(scroll: &gtk::ScrolledWindow) -> bool {
    let overlay = scroll
        .ancestor(gtk::Overlay::static_type())
        .and_downcast::<gtk::Overlay>()
        .unwrap_or_else(gtk::Overlay::new);
    let marker = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    marker.add_css_class("autoscroll-anchor");
    marker.set_can_target(false);
    marker.set_visible(false);
    overlay.add_overlay(&marker);
    let vertical = scroll.vadjustment();
    if !scrollable(&vertical) && !scrollable(&scroll.hadjustment()) {
        vertical.set_upper(vertical.lower() + vertical.page_size() + 200.0);
    }
    let state = Rc::new(AutoScroll {
        scroll: scroll.downgrade(),
        overlay: overlay.downgrade(),
        marker,
        anchor: Cell::new((0.0, 0.0)),
        pointer: Cell::new((0.0, 0.0)),
        frames: RefCell::new(None),
    });
    state.start((10.0, 10.0))
}






pub(super) fn advance(adjustment: &gtk::Adjustment, step: f64) {
    if step == 0.0 {
        return;
    }
    let upper = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    let target = (adjustment.value() + step).clamp(adjustment.lower(), upper);
    if (target - adjustment.value()).abs() >= f64::EPSILON {
        adjustment.set_value(target);
    }
}


/// The collection view holding keyboard focus, with the scrolling area it moves in.
pub(super) fn focused_collection(
    focused: &gtk::Widget,
) -> Option<(gtk::Widget, gtk::ScrolledWindow)> {
    let mut current = Some(focused.clone());
    while let Some(widget) = current {
        if widget.is::<gtk::ListView>() || widget.is::<gtk::GridView>() {
            let scroll = widget
                .ancestor(gtk::ScrolledWindow::static_type())
                .and_downcast::<gtk::ScrolledWindow>()?;
            return Some((widget, scroll));
        }
        current = widget.parent();
    }
    None
}

/// One page move of a collection view: how many entries the focus travels and how
/// far that is on screen.
pub(super) struct Page {
    pub items: usize,
    distance: f64,
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CursorMotion {
    /// One entry. Focus-follow scrolling already keeps the cursor in sight.
    Step,
    Page,
    Jump,
}

/// Grid pages scroll by pixels; list rows and jumps follow the cursor, not the fill.
pub(super) fn reveal_cursor(
    view: &gtk::Widget,
    scroll: &gtk::ScrolledWindow,
    direction: i32,
    motion: CursorMotion,
    position: Option<u32>,
) {
    let grid = view.is::<gtk::GridView>();
    if motion == CursorMotion::Page && grid {
        advance(
            &scroll.vadjustment(),
            f64::from(direction) * page(view, scroll).distance,
        );
        return;
    }
    // GridView `scroll_to` relies on stale cell estimates; the pane's own focus
    // follow brings a single grid step into view.
    if motion == CursorMotion::Step && grid {
        return;
    }
    if scroll.child().is_some_and(|child| &child == view)
        && let Some(position) = position.or_else(|| selected_position(view))
    {
        scroll_to_item(view, position);
        return;
    }
    let distance = match motion {
        CursorMotion::Step => return,
        CursorMotion::Page => page(view, scroll).distance,
        CursorMotion::Jump => f64::MAX,
    };
    advance(&scroll.vadjustment(), f64::from(direction) * distance);
}


fn selected_position(view: &gtk::Widget) -> Option<u32> {
    let model = match (
        view.downcast_ref::<gtk::ListView>(),
        view.downcast_ref::<gtk::GridView>(),
    ) {
        (Some(list), _) => list.model(),
        (_, Some(grid)) => grid.model(),
        _ => None,
    }?;
    let selection = model.selection();
    (!selection.is_empty()).then(|| selection.minimum())
}

fn scroll_to_item(view: &gtk::Widget, position: u32) {
    if let Some(list) = view.downcast_ref::<gtk::ListView>() {
        list.scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
    } else if let Some(grid) = view.downcast_ref::<gtk::GridView>() {
        grid.scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
    }
}

/// How far one `Page Up` or `Page Down` should move `view`, derived from the items
/// it currently shows.
pub(super) fn page(view: &gtk::Widget, scroll: &gtk::ScrolledWindow) -> Page {
    let page_size = scroll.vadjustment().page_size();
    let Some((item_height, columns)) = item_geometry(view) else {
        return Page {
            items: 1,
            distance: page_size,
        };
    };
    let rows = rows_per_page(page_size, item_height);
    Page {
        items: rows * columns,
        distance: rows as f64 * item_height,
    }
}

fn item_geometry(view: &gtk::Widget) -> Option<(f64, usize)> {
    if view.is::<gtk::GridView>() {
        return grid_geometry(view);
    }
    list_row_geometry(view)
}

/// Grid columns follow the live allocation, so opening the preview pane or
/// dragging the thumbnail slider changes the page immediately. Cell size comes
/// from the card's size request / measure, not recycled allocated bounds.
fn grid_geometry(view: &gtk::Widget) -> Option<(f64, usize)> {
    let (col_pitch, row_pitch) = grid_cell_pitch(view)?;
    let width = f64::from(view.width().max(0));
    let (min_columns, max_columns) = view
        .downcast_ref::<gtk::GridView>()
        .map(|grid| (grid.min_columns(), grid.max_columns()))
        .unwrap_or((1, 20));
    let columns = grid_page_columns(width, col_pitch, min_columns, max_columns);
    Some((row_pitch, columns))
}

fn grid_cell_pitch(view: &gtk::Widget) -> Option<(f64, f64)> {
    let mut child = view.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if !widget.is_visible() {
            continue;
        }
        if let Some(pitch) = cell_pitch_from_widget(&widget) {
            return Some(pitch);
        }
    }
    None
}

fn cell_pitch_from_widget(widget: &gtk::Widget) -> Option<(f64, f64)> {
    let (_, nat_w, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
    let (_, nat_h, _, _) = widget.measure(gtk::Orientation::Vertical, -1);
    if nat_w > 0 && nat_h > 0 {
        return Some((f64::from(nat_w), f64::from(nat_h)));
    }
    let mut inner = widget.first_child();
    while let Some(child) = inner {
        if child.has_css_class("grid-card") {
            let width = child.width_request();
            let height = child.height_request();
            if width > 0 && height > 0 {
                return Some((f64::from(width), f64::from(height)));
            }
        }
        inner = child.next_sibling();
    }
    None
}

fn grid_page_columns(width: f64, col_pitch: f64, min_columns: u32, max_columns: u32) -> usize {
    let min = min_columns.max(1);
    let max = max_columns.max(min);
    if col_pitch <= 0.0 || width <= 0.0 {
        return min as usize;
    }
    ((width / col_pitch).floor() as u32).clamp(min, max) as usize
}

fn list_row_geometry(view: &gtk::Widget) -> Option<(f64, usize)> {
    let mut child = view.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if !widget.is_visible() {
            continue;
        }
        let (_, nat_h, _, _) = widget.measure(gtk::Orientation::Vertical, -1);
        if nat_h > 0 {
            return Some((f64::from(nat_h), 1));
        }
        if let Some(bounds) = widget.compute_bounds(view)
            && bounds.height() > 0.0
        {
            return Some((f64::from(bounds.height()), 1));
        }
    }
    None
}

/// Rows to move for one page, keeping a row of overlap so the reader retains
/// context across the jump.
fn rows_per_page(page_size: f64, item_height: f64) -> usize {
    if item_height <= 0.0 {
        return 1;
    }
    let rows = (page_size / item_height).floor().max(1.0) as usize;
    rows.saturating_sub(1).max(1)
}

#[cfg(test)]
mod tests;
