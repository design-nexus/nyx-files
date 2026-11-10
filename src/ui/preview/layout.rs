// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::browser::COLUMN_WIDTH;

const MIN_SPLIT_PREVIEW_WIDTH: i32 = 240;
/// The divider only takes its own pixel so the column beside it keeps its resize
/// edge; this strip inside the preview is where the divider is grabbed instead.
pub(super) const RESIZE_GRIP_WIDTH: i32 = 6;

#[derive(Default)]
pub(super) struct SplitSizing {
    manual_width: Cell<Option<i32>>,
    resizing: Cell<bool>,
    /// A pointer, not the keyboard, is dragging the divider.
    dragging: Cell<bool>,
    /// The divider position a resize restored, which GTK reports back afterwards.
    restored_position: Cell<Option<i32>>,
    minimum_outline: RefCell<Option<gtk::Box>>,
    grip_hint: Rc<crate::ui::resize_feedback::EdgeHint>,
    suspended: Cell<bool>,
    resume_media: Cell<bool>,
    reload_on_resume: Cell<bool>,
}

impl SplitSizing {

    pub(super) fn close(&self) {
        self.resizing.set(false);
        self.suspended.set(false);
        self.resume_media.set(false);
        self.reload_on_resume.set(false);
    }

    pub(super) fn defer_load(&self) {
        self.reload_on_resume.set(true);
        self.resume_media.set(false);
    }

    pub(super) fn play_or_defer(&self, media: &gtk::MediaStream) {
        if self.suspended.get() {
            self.resume_media.set(true);
        } else {
            media.play();
        }
    }

    pub(super) fn is_suspended(&self) -> bool {
        self.suspended.get()
    }
}

#[derive(Clone, Copy)]
struct Geometry {
    available: i32,
    occupied: i32,
    separator: i32,
}

impl Geometry {
    fn can_show_preview(self) -> bool {
        self.available - self.separator >= MIN_SPLIT_PREVIEW_WIDTH
    }

    fn maximum_width(self) -> i32 {
        (self.available - self.separator).max(1)
    }

    fn minimum_width(self, manual: bool) -> i32 {
        let minimum = if manual { COLUMN_WIDTH } else { MIN_WIDTH };
        minimum.min(self.maximum_width())
    }

    fn desired_width(self, manual: Option<i32>) -> i32 {
        let free = (self.available - self.separator - self.occupied).max(0);
        let desired =
            manual.unwrap_or_else(|| free.saturating_mul(9).saturating_div(10).min(MAX_WIDTH));
        desired.clamp(self.minimum_width(manual.is_some()), self.maximum_width())
    }

    fn preview_width(self, manual: Option<i32>) -> i32 {
        self.desired_width(manual).max(MIN_SPLIT_PREVIEW_WIDTH)
    }

    fn position(self, manual: Option<i32>) -> i32 {
        self.available - self.separator - self.preview_width(manual)
    }

}

pub(crate) fn separator(split: &gtk::Paned) -> Option<gtk::Widget> {
    let mut child = split.first_child();
    while let Some(widget) = child {
        if widget.css_name() == "separator" {
            return Some(widget);
        }
        child = widget.next_sibling();
    }
    None
}

pub(crate) fn separator_width(split: &gtk::Paned) -> i32 {
    separator(split).map_or(0, |handle| {
        handle.measure(gtk::Orientation::Horizontal, -1).0
    })
}

impl PreviewDrawer {

    /// Hosts the preview at the end of `split`, without an old browser view:
    /// sizing, the resize grip and the preview style follow the split alone.
    pub(crate) fn attach_split_only(&self, split: &gtk::Paned) {
        self.state.split.replace(Some(split.clone()));
        self.state.panel_split.replace(Some(split.clone()));
        split.set_end_child(Some(&self.state.slot));
        self.state.revealer.set_visible(self.state.is_enabled());
        self.state.slot.set_visible(self.state.is_enabled());
        let weak = Rc::downgrade(&self.state);
        crate::ui::frame::tick_while_active(split, 60, move |split| {
            let Some(state) = weak.upgrade() else {
                return;
            };
            if !state.floating.get() && state.is_enabled() {
                state.sync_split(split);
            }
        });
        let weak = Rc::downgrade(&self.state);
        split.connect_unmap(move |_| {
            let Some(state) = weak.upgrade().filter(|state| state.is_enabled()) else {
                return;
            };
            // A popup outliving its hidden tab would keep showing that tab's file.
            if state.floating.get() {
                state.close();
            } else {
                state.suspend_panel();
            }
        });
        let weak = Rc::downgrade(&self.state);
        split.connect_unrealize(move |_| {
            if let Some(state) = weak.upgrade() {
                state.stop();
            }
        });
        if let Some(handle) = separator(split) {
            handle.set_cursor_from_name(Some("col-resize"));
        }
        install_resize(split, &self.state);
        let weak = Rc::downgrade(&self.state);
        crate::ui::preferences::PreferenceManager::shared().bind_preference(
            split,
            |manager| manager.preview_style(),
            move |_, style| {
                if let Some(state) = weak.upgrade() { state.change_presentation(style); }
            },
        );

        install_resize_grip(split, &self.state);
    }
}

impl PreviewState {
    pub(super) fn selected_entry(&self) -> (Option<FileEntry>, Option<usize>) {
        (
            preview_target(self.current.borrow().clone()),
            self.current_depth.get(),
        )
    }

    fn geometry(&self, split: &gtk::Paned) -> Geometry {
        let available = split.width();
        Geometry {
            available,
            occupied: available.saturating_sub(DEFAULT_WIDTH),
            separator: separator_width(split),
        }
    }

    pub(super) fn can_show_in(&self, split: &gtk::Paned) -> bool {
        split.is_mapped() && self.geometry(split).can_show_preview()
    }

    pub(super) fn show_panel(&self) {
        self.revealer.set_transition_duration(0);
        self.pane.set_width_request(0);
        self.slot.set_visible(true);
        self.revealer.set_visible(true);
        self.revealer.set_reveal_child(true);
    }

    pub(super) fn hide_panel(&self) {
        self.finish_hide(self.hide_intent());
    }

    /// Whether the listing needs focus restored once the panel is gone.
    pub(super) fn hide_intent(&self) -> bool {
        self.pane
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focused| {
                focused == self.pane
                    || focused.is_ancestor(&self.pane)
                    // GTK may focus a divider while allocating a smaller split.
                    || self.split.borrow().as_ref().is_some_and(|split| split.has_focus())
            })
    }

    /// Pins the pane to its resting width while the divider sweeps, so the slot
    /// clips a fully laid out panel instead of reflowing it every frame.
    fn pin_pane_width(&self, split: &gtk::Paned) {
        self.pane.set_width_request(
            self.geometry(split)
                .preview_width(self.sizing.manual_width.get()),
        );
    }

    fn finish_hide(&self, restore_listing_focus: bool) {
        if self.floating.get()
            && let Some(window) = self.popup.borrow().clone()
        {
            window.set_visible(false);
        }
        self.revealer.set_transition_duration(0);
        self.revealer.set_reveal_child(false);
        self.revealer.set_visible(false);
        if let Some(split) = self.split.borrow().as_ref() {
            self.slot.set_visible(false);
            split.set_resize_start_child(true);
            split.set_resize_end_child(false);
            split.set_position(split.width());
        }
        if restore_listing_focus
            && let Some(split) = self.split.borrow().as_ref()
            && let Some(host) = self.host()
        {
            split.add_tick_callback(move |_, _| {
                (host.focus_listing)();
                glib::ControlFlow::Break
            });
        }
    }

    fn suspend_panel(&self) {
        if self.sizing.suspended.replace(true) {
            return;
        }
        self.animation_generation
            .set(self.animation_generation.get().saturating_add(1));
        self.animating.set(false);
        self.sizing.resizing.set(false);
        let media = self.media.borrow().clone();
        self.sizing
            .resume_media
            .set(media.as_ref().is_some_and(|media| media.is_playing()));
        if let Some(media) = media {
            media.pause();
        }
        self.hide_panel();
    }

    pub(super) fn sync_split(self: &Rc<Self>, split: &gtk::Paned) {
        let geometry = self.geometry(split);
        self.slot.set_width_request(0);
        if self.current.borrow().is_none() {
            if self.revealer.reveals_child() {
                self.hide_panel();
            }
            self.sizing.suspended.set(false);
            return;
        }
        if !split.is_mapped() || !geometry.can_show_preview() {
            self.suspend_panel();
            return;
        }
        if self.animating.get() || self.sizing.resizing.get() {
            return;
        }
        // GTK allocates after this tick; free-space previews must absorb window
        // growth instead of correcting the browser's divider on the next frame.
        let manual = self.sizing.manual_width.get();
        let position = geometry.position(manual);
        split.set_resize_start_child(true);
        split.set_resize_end_child(false);
        let restored = self.sizing.suspended.replace(false);
        if restored || !self.revealer.reveals_child() {
            self.show_panel();
        }
        let minimum = geometry.minimum_width(manual.is_some());
        if self.pane.width_request() != minimum || split.position() != position {
            self.pane.set_width_request(minimum);
            split.set_position(position);
        }
        if self.sizing.reload_on_resume.replace(false) {
            let entry = self.current.borrow().clone();
            if let Some(entry) = entry {
                self.load(entry, 0);
            }
        } else if restored && self.sizing.resume_media.replace(false) {
            let media = self.media.borrow().clone();
            if let Some(media) = media {
                media.play();
            }
        }
        if restored {
            self.resume_keyboard_claim();
        }
    }

    pub(super) fn opening_width(&self, available: i32) -> i32 {
        self.split
            .borrow()
            .as_ref()
            .map_or(DEFAULT_WIDTH.min(available), |split| {
                self.geometry(split)
                    .preview_width(self.sizing.manual_width.get())
            })
    }

    /// `on_settled` runs once, when the panel has finished opening or closing.
    pub(super) fn animate_reveal(
        self: &Rc<Self>,
        split: &gtk::Paned,
        expanded: bool,
        on_settled: impl FnOnce(&Rc<Self>) + 'static,
    ) {
        let geometry = self.geometry(split);
        if expanded && !geometry.can_show_preview() {
            self.show_panel();
            self.sync_split(split);
            on_settled(self);
            return;
        }
        let animation_id = self.animation_generation.get().saturating_add(1);
        self.animation_generation.set(animation_id);
        let (start, restore_listing_focus) = if expanded {
            // A reopen during the slide out turns back from where the drawer is.
            let start = if self.animating.get() {
                split.position()
            } else {
                split.width()
            };
            self.show_panel();
            // The animation owns the divider, including after a resize while the pane was hidden.
            split.set_resize_start_child(false);
            split.set_resize_end_child(true);
            self.pin_pane_width(split);
            split.set_position(start);
            (start, false)
        } else {
            self.pin_pane_width(split);
            (split.position(), self.hide_intent())
        };
        let target = move |state: &Self, split: &gtk::Paned| {
            if expanded {
                state
                    .geometry(split)
                    .position(state.sizing.manual_width.get())
            } else {
                split.width()
            }
        };
        let settle = move |state: &Rc<Self>, split: &gtk::Paned| {
            state.animating.set(false);
            state.pane.set_width_request(0);
            if expanded {
                state.sync_split(split);
            } else {
                state.finish_hide(restore_listing_focus);
            }
        };

        if !super::super::motion::animations_enabled() || start <= 0 {
            split.set_position(target(self, split));
            settle(self, split);
            on_settled(self);
            return;
        }

        self.animating.set(true);
        let started = Instant::now();
        let weak = Rc::downgrade(self);
        let on_settled = RefCell::new(Some(on_settled));
        split.add_tick_callback(move |split, _| {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if state.animation_generation.get() != animation_id {
                return glib::ControlFlow::Break;
            }
            let progress =
                (started.elapsed().as_secs_f64() / TRANSITION.as_secs_f64()).clamp(0.0, 1.0);
            let eased = super::super::motion::emphasized_deceleration(progress);
            let end = target(&state, split);
            let position = (f64::from(start) + f64::from(end - start) * eased).round() as i32;
            split.set_position(position);
            state.pin_pane_width(split);
            if progress < 1.0 {
                return glib::ControlFlow::Continue;
            }
            settle(&state, split);
            if let Some(callback) = on_settled.borrow_mut().take() {
                callback(&state);
            }
            glib::ControlFlow::Break
        });
    }

    pub(super) fn resize_preview(
        self: &Rc<Self>,
        split: &gtk::Paned,
        position: i32,
    ) -> Option<i32> {
        let geometry = self.geometry(split);
        if !geometry.can_show_preview() {
            return None;
        }
        let width = (geometry.available - geometry.separator - position)
            .clamp(geometry.minimum_width(true), geometry.maximum_width());
        self.sizing.manual_width.set(Some(width));
        self.sync_split(split);
        Some(width)
    }

    /// Every divider drag sets the session minimum. When it asks for less than the
    /// space the preview fills, the panel stays put, so the outline shows that minimum.
    fn show_minimum_outline(&self, split: &gtk::Paned, width: i32) {
        let overlay = crate::ui::modal::window_overlay(split);
        let bounds = overlay
            .as_ref()
            .and_then(|overlay| split.compute_bounds(overlay));
        let (Some(overlay), Some(bounds)) = (overlay, bounds) else {
            return;
        };
        if !self.revealer.reveals_child() {
            self.remove_minimum_outline();
            return;
        }
        let outline = self
            .sizing
            .minimum_outline
            .borrow_mut()
            .get_or_insert_with(|| {
                let outline = gtk::Box::new(gtk::Orientation::Vertical, 0);
                outline.add_css_class("preview-minimum-outline");
                outline.set_can_target(false);
                outline.set_halign(gtk::Align::Start);
                outline.set_valign(gtk::Align::Start);
                let label = super::super::resize_feedback::caption(&crate::i18n::tr(
                    "Preview panel minimum width",
                ));
                label.set_halign(gtk::Align::Center);
                outline.append(&label);
                overlay.add_overlay(&outline);
                outline
            })
            .clone();
        outline.set_margin_start((bounds.x() + bounds.width()).round() as i32 - width);
        outline.set_margin_top(bounds.y().round() as i32);
        outline.set_size_request(width, bounds.height().round() as i32);
    }

    fn fade_minimum_outline(&self) {
        if let Some(outline) = self.sizing.minimum_outline.take() {
            super::super::resize_feedback::fade_out(&outline);
        }
    }

    fn can_drag_divider(&self) -> bool {
        self.is_enabled() && !self.sizing.is_suspended()
    }

    fn begin_divider_drag(&self, split: &gtk::Paned) {
        self.animation_generation
            .set(self.animation_generation.get().saturating_add(1));
        self.animating.set(false);
        self.sizing.resizing.set(true);
        self.sizing.dragging.set(true);
        // The outline names the minimum from here on.
        self.sizing.grip_hint.hide();
        let minimum = self.geometry(split).minimum_width(true);
        self.pane.set_width_request(minimum);
        self.slot.set_width_request(minimum);
    }

    fn end_divider_drag(&self) {
        self.sizing.resizing.set(false);
        if self.sizing.dragging.replace(false) {
            self.fade_minimum_outline();
        }
    }

    fn remove_minimum_outline(&self) {
        if let Some(outline) = self.sizing.minimum_outline.take()
            && let Some(overlay) = outline.parent().and_downcast::<gtk::Overlay>()
        {
            overlay.remove_overlay(&outline);
        }
    }
}

/// Moves the divider from the grip, measuring in window coordinates because the
/// grip itself moves with the divider.
fn install_resize_grip(split: &gtk::Paned, state: &Rc<PreviewState>) {
    // A hidden preview has no width to set, so only a docked one offers a resize.
    let docked_split = split.downgrade();
    let grip = state.resize_grip.clone();
    let hint = state.sizing.grip_hint.clone();
    let follow_reveal = move |revealer: &gtk::Revealer| {
        let docked = revealer.reveals_child();
        grip.set_visible(docked);
        if !docked {
            hint.hide();
        }
        if let Some(handle) = docked_split.upgrade().and_then(|split| separator(&split)) {
            // GTK does not pick a divider without a cursor, so it cannot be dragged.
            handle.set_cursor_from_name(docked.then_some("col-resize"));
        }
    };
    follow_reveal(&state.revealer);
    state.revealer.connect_reveal_child_notify(follow_reveal);

    install_edge_hover(split, state);

    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    let origin = Rc::new(Cell::new(None::<(i32, f64)>));
    let weak = Rc::downgrade(state);
    let begun_split = split.downgrade();
    let begun = origin.clone();
    drag.connect_drag_begin(move |gesture, _, _| {
        let pointer = gesture.current_event().and_then(|event| event.position());
        let (Some(state), Some(split), Some((pointer_x, _))) =
            (weak.upgrade(), begun_split.upgrade(), pointer)
        else {
            gesture.set_state(gtk::EventSequenceState::Denied);
            return;
        };
        if !state.can_drag_divider() {
            gesture.set_state(gtk::EventSequenceState::Denied);
            return;
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
        state.begin_divider_drag(&split);
        begun.set(Some((split.position(), pointer_x)));
    });
    let moved_split = split.downgrade();
    let moved = origin.clone();
    drag.connect_drag_update(move |gesture, _, _| {
        let (Some(split), Some((start, pointer_start))) = (moved_split.upgrade(), moved.get())
        else {
            return;
        };
        if let Some((pointer_x, _)) = gesture.current_event().and_then(|event| event.position()) {
            split.set_position(start + (pointer_x - pointer_start).round() as i32);
        }
    });
    let weak = Rc::downgrade(state);
    let ended = origin.clone();
    drag.connect_drag_end(move |_, _, _| {
        if ended.take().is_some()
            && let Some(state) = weak.upgrade()
        {
            state.end_divider_drag();
        }
    });
    let weak = Rc::downgrade(state);
    drag.connect_cancel(move |_, _| {
        if origin.take().is_some()
            && let Some(state) = weak.upgrade()
        {
            state.end_divider_drag();
        }
    });
    state.resize_grip.add_controller(drag);
}

/// The divider line also drags the divider, so it shares the grip's hover: each
/// lights the other, and both show one caption.
fn install_edge_hover(split: &gtk::Paned, state: &Rc<PreviewState>) {
    let Some(divider) = separator(split) else {
        return;
    };
    let hovers = [
        gtk::EventControllerMotion::new(),
        gtk::EventControllerMotion::new(),
    ];
    let watched: Rc<[glib::WeakRef<gtk::EventControllerMotion>]> =
        hovers.iter().map(|hover| hover.downgrade()).collect();
    let partners = [
        state.resize_grip.clone().upcast::<gtk::Widget>(),
        divider.clone(),
    ];
    for (hover, partner) in hovers.iter().zip(partners) {
        let partner_for_enter = partner.downgrade();
        let hovered = Rc::downgrade(state);
        hover.connect_enter(move |_, _, _| {
            let Some(state) = hovered.upgrade() else {
                return;
            };
            if !state.resize_grip.is_visible() {
                return;
            }
            if let Some(partner) = partner_for_enter.upgrade() {
                partner.add_css_class("resize-hover");
            }
            let grip = state.resize_grip.downgrade();
            state.sizing.grip_hint.hover(
                &state.resize_grip,
                "Preview panel minimum width",
                Rc::new(move |overlay| {
                    let bounds = grip.upgrade()?.compute_bounds(overlay)?;
                    Some((bounds.x(), bounds.y()))
                }),
            );
        });
        let partner = partner.downgrade();
        let left = Rc::downgrade(state);
        let watched = watched.clone();
        hover.connect_leave(move |_| {
            if let Some(partner) = partner.upgrade() {
                partner.remove_css_class("resize-hover");
            }
            let left = left.clone();
            let watched = watched.clone();
            // Crossing between the line and the grip leaves one before entering the other.
            glib::idle_add_local_once(move || {
                let still_on_edge = watched.iter().any(|hover| {
                    hover
                        .upgrade()
                        .is_some_and(|hover| hover.contains_pointer())
                });
                if let Some(state) = left.upgrade()
                    && !still_on_edge
                    && !state.sizing.dragging.get()
                {
                    state.sizing.grip_hint.hide();
                }
            });
        });
    }
    let [divider_hover, grip_hover] = hovers;
    divider.add_controller(divider_hover);
    state.resize_grip.add_controller(grip_hover);
}

fn install_resize(split: &gtk::Paned, state: &Rc<PreviewState>) {
    // Observe input without competing with GtkPaned's own drag gesture.
    let pointer = gtk::EventControllerLegacy::new();
    pointer.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(state);
    pointer.connect_event(move |controller, event| {
        let Some(state) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let Some(split) = controller.widget().and_downcast::<gtk::Paned>() else {
            return glib::Propagation::Proceed;
        };
        match event.event_type() {
            gtk::gdk::EventType::ButtonPress | gtk::gdk::EventType::TouchBegin
                if (event.event_type() == gtk::gdk::EventType::TouchBegin
                    || event
                        .downcast_ref::<gtk::gdk::ButtonEvent>()
                        .is_some_and(|e| e.button() == 1))
                    && state.can_drag_divider()
                    && on_separator(&split, event) =>
            {
                state.begin_divider_drag(&split);
            }
            gtk::gdk::EventType::ButtonRelease
            | gtk::gdk::EventType::TouchEnd
            | gtk::gdk::EventType::TouchCancel
            | gtk::gdk::EventType::GrabBroken => state.end_divider_drag(),
            _ => {}
        }
        glib::Propagation::Proceed
    });
    split.add_controller(pointer);
    let weak = Rc::downgrade(state);
    split.connect_position_notify(move |split| {
        if let Some(state) = weak.upgrade()
            && state.is_enabled()
            && state.sizing.restored_position.take() != Some(split.position())
            && state.sizing.resizing.replace(false)
        {
            let dragged = split.position();
            let requested = state.resize_preview(split, dragged);
            if split.position() != dragged {
                // Filling the free space put the divider back; that echo is not a resize.
                state.sizing.restored_position.set(Some(split.position()));
            }
            if state.sizing.dragging.get()
                && let Some(width) = requested
            {
                state.show_minimum_outline(split, width);
            }
            state.sizing.resizing.set(true);
        }
    });

    // Unhandled browser keys also reach GtkPaned; only handle-focused actions are resizes.
    let weak = Rc::downgrade(state);
    split.connect_move_handle(move |split, _| {
        if split.has_focus() {
            if let Some(state) = weak.upgrade()
                && state.can_drag_divider()
            {
                let minimum = state.geometry(split).minimum_width(true);
                state.pane.set_width_request(minimum);
                state.slot.set_width_request(minimum);
            }
            remember_keyboard_width(weak.clone());
        }
        false
    });
    let weak = Rc::downgrade(state);
    split.connect_cancel_position(move |split| {
        if split.has_focus() {
            remember_keyboard_width(weak.clone());
        }
        false
    });
}

fn on_separator(split: &gtk::Paned, event: &gtk::gdk::Event) -> bool {
    let Some((x, y)) = event.position() else {
        return false;
    };
    let Some(native) = split.native() else {
        return false;
    };
    let (dx, dy) = native.surface_transform();
    let native: gtk::Widget = native.upcast();
    let Some(point) = native.compute_point(
        split,
        &gtk::graphene::Point::new((x + dx) as f32, (y + dy) as f32),
    ) else {
        return false;
    };
    split.pick(
        f64::from(point.x()),
        f64::from(point.y()),
        gtk::PickFlags::DEFAULT,
    ) == separator(split)
}

fn remember_keyboard_width(weak: std::rc::Weak<PreviewState>) {
    let Some(state) = weak.upgrade().filter(|state| state.can_drag_divider()) else {
        return;
    };
    let Some(split) = state.split.borrow().clone() else {
        return;
    };
    let before = split.position();
    state.sizing.resizing.set(true);
    // Wait for GTK's default action handler before resuming automatic layout.
    glib::idle_add_local_once(move || {
        if let Some(state) = weak.upgrade() {
            state.sizing.resizing.set(false);
            if state.is_enabled() && split.position() != before {
                state.resize_preview(&split, split.position());
            }
        }
    });
}
