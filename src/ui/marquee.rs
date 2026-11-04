// SPDX-License-Identifier: MIT

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::glib;
use gtk::graphene;
use gtk::prelude::*;

thread_local! {
    static UPDATING_SELECTION: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn is_updating_selection() -> bool {
    UPDATING_SELECTION.get()
}



/// Visits every bound item of a collection view as `(position, widget)`, dropping
/// entries whose widgets have been recycled.
pub(super) type ItemVisitor = Rc<dyn Fn(&mut dyn FnMut(u32, &gtk::Widget))>;
pub(super) type ItemPredicate = Rc<dyn Fn(&gtk::Widget, f64, f64) -> bool>;

/// One collection view a drag can select in. A grouped view contributes one target
/// per group, since each group renders through its own view and selection model.
pub(super) struct MarqueeTarget {
    pub selection: gtk::MultiSelection,
    pub visit_items: ItemVisitor,
}

/// Targets shared with the caller, so a view that rebuilds its groups can replace
/// them without reinstalling the drag.
pub(super) type MarqueeTargets = Rc<RefCell<Vec<MarqueeTarget>>>;





#[derive(Clone)]
pub(super) struct Marquee {
    state: Rc<MarqueeState>,
    gestures: Rc<RefCell<Vec<gtk::GestureDrag>>>,
}

struct MarqueeState {
    // Weak: the drag gesture lives on the surface, and capturing these widgets
    // would pin the collection (and its model) after a mode switch.
    view: glib::WeakRef<gtk::Widget>,
    scroll: glib::WeakRef<gtk::ScrolledWindow>,
    overlay: glib::WeakRef<gtk::Overlay>,
    band: gtk::Box,
    targets: MarqueeTargets,
    is_item: ItemPredicate,
    active: Cell<bool>,
    dragging: Cell<bool>,
    clear_on_click: Cell<bool>,
    clear_selection: Rc<dyn Fn()>,
    allow_drag: Rc<Cell<bool>>,
    /// Anchor in scroll-content coordinates. Native GtkScrollable views move their
    /// rows internally, whereas GtkViewport moves its child; neither may move the anchor.
    anchor: Cell<(f64, f64)>,
    /// Last pointer position in the scrolled window's coordinates, which do not
    /// move while the content scrolls.
    pointer: Cell<(f64, f64)>,
    // Surface coordinates distinguish pointer motion from a column moving underneath it.
    press_position: Cell<Option<(f64, f64)>>,
    /// Selection of every target as the drag began, one entry per target.
    initial: RefCell<Vec<gtk::Bitset>>,
    /// Keep geometry after virtualization unbinds a row so extending or retracting
    /// the band still resolves against items scrolled out of the viewport.
    item_bounds: RefCell<Vec<BTreeMap<u32, graphene::Rect>>>,
    modifiers: Cell<(bool, bool)>,
    auto_scroll: RefCell<Option<glib::SourceId>>,
    frame_handler: RefCell<Option<(glib::WeakRef<gtk::gdk::FrameClock>, glib::SignalHandlerId)>>,
    refresh_pending: Cell<bool>,
    finishing: Cell<bool>,
}


impl Marquee {


}








impl MarqueeState {



















}







#[cfg(test)]
mod tests;
