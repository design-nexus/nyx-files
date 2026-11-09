// SPDX-License-Identifier: MIT

use crate::ui::blur::BlurBin;
use crate::ui::controls::{dialog_text, message_dialog_description};
use crate::ui::keys::{self, bindings::ESCAPE_ANY};
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

pub(super) mod layout;



struct ModalFocusOrigin {
    widget: Option<glib::WeakRef<gtk::Widget>>,
    restore: RefCell<Option<Rc<dyn Fn()>>>,
    /// False when the first dialog of a chain changes the listing; dialogs chained on
    /// it inherit that and return focus through the window fallback too.
    allow_widget: bool,
}

#[derive(Default)]
struct LayerFocus {
    /// The focused widget when the layer was added. A layer closed while another
    /// modal stays open returns focus here if it lies inside that modal.
    opener: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    /// Where the layer was added; a removed layer no longer knows its window.
    host: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    /// Whether the window showed focus rings when the dismissal began.
    focus_visible: Cell<bool>,
}

type FocusOrigins = Vec<(glib::WeakRef<gtk::Widget>, Rc<ModalFocusOrigin>)>;
type FocusFallbacks = Vec<(glib::WeakRef<gtk::Window>, Rc<dyn Fn()>)>;
type LayerFocusEntries = Vec<(glib::WeakRef<gtk::Widget>, Rc<LayerFocus>)>;

thread_local! {
    static MODAL_FOCUS_ORIGINS: RefCell<FocusOrigins> = const { RefCell::new(Vec::new()) };
    static MODAL_FOCUS_FALLBACKS: RefCell<FocusFallbacks> = const { RefCell::new(Vec::new()) };
    static LAYER_FOCUS: RefCell<LayerFocusEntries> = const { RefCell::new(Vec::new()) };
}

fn layer_focus(layer: &gtk::Widget) -> Rc<LayerFocus> {
    let existing = LAYER_FOCUS.with(|entries| {
        let mut entries = entries.borrow_mut();
        entries.retain(|(candidate, _)| candidate.upgrade().is_some());
        entries
            .iter()
            .find(|(candidate, _)| {
                candidate
                    .upgrade()
                    .is_some_and(|candidate| candidate == *layer)
            })
            .map(|(_, focus)| focus.clone())
    });
    if let Some(focus) = existing {
        return focus;
    }
    let focus = Rc::new(LayerFocus::default());
    let tracked = focus.clone();
    layer.connect_sensitive_notify(move |layer| {
        // A dismissal makes the layer insensitive while its control has focus. GTK
        // then drops focus and, on the key release, hides focus rings, so the
        // window's ring state is captured before that.
        if layer.is_sensitive() {
            tracked.focus_visible.set(false);
        } else if layer.has_css_class("dismissing")
            && let Some(window) = layer.root().and_downcast::<gtk::Window>()
        {
            tracked.focus_visible.set(window.gets_focus_visible());
        }
    });
    LAYER_FOCUS.with(|entries| {
        entries
            .borrow_mut()
            .push((layer.downgrade(), focus.clone()));
    });
    focus
}

/// Shows focus rings again after a keyboard dismissal hid them, once focus is back.
fn rearm_focus_ring(window: &gtk::Window, layer: &LayerFocus) {
    if layer.focus_visible.get() && gtk::prelude::RootExt::focus(window).is_some() {
        window.set_focus_visible(true);
    }
}

/// Returns focus to the opener of a layer closed over `under`, a modal that stays
/// open, unless something inside `under` already took focus. Without a usable opener,
/// such as a row the dialog's action re-rendered, `under`'s first focusable control
/// takes it. The window's restore order applies only once no modal is left.
fn restore_focus_under(window: &gtk::Window, under: &gtk::Widget, layer: &LayerFocus) {
    let live = gtk::prelude::RootExt::focus(window)
        .is_some_and(|focus| focus.is_mapped() && focus != *under);
    if live {
        return;
    }
    let opener = layer
        .opener
        .borrow()
        .as_ref()
        .and_then(glib::WeakRef::upgrade)
        .filter(|opener| {
            (opener == under || opener.is_ancestor(under))
                && shown_or_revealing(opener)
                && opener.is_sensitive()
        });
    let moved = match opener {
        Some(opener) => opener.grab_focus(),
        None => focus_first_control(under),
    };
    if moved {
        rearm_focus_ring(window, layer);
    }
}

fn focus_first_control(root: &gtk::Widget) -> bool {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if widget.is_visible() && widget.is_sensitive() {
            if widget.is_focusable() && widget.is_mapped() && widget.grab_focus() {
                return true;
            }
            if focus_first_control(&widget) {
                return true;
            }
        }
        child = widget.next_sibling();
    }
    false
}

pub(in crate::ui) fn is_closing_layer(widget: &gtk::Widget) -> bool {
    widget.has_css_class("app-modal-layer") && widget.has_css_class("dismissing")
}

/// The focused widget when a layer opens. A layer opened while another closes, such
/// as an error after a confirmation, finds focus gone or inside the closing layer,
/// so it takes over the closing layer's opener instead.
fn layer_opener(host: &gtk::Widget, window: &gtk::Window) -> Option<glib::WeakRef<gtk::Widget>> {
    let focus = gtk::prelude::RootExt::focus(window);
    let closing = match &focus {
        Some(focus) => {
            let mut current = Some(focus.clone());
            while let Some(widget) = current.as_ref().filter(|widget| !is_closing_layer(widget)) {
                current = widget.parent();
            }
            current
        }
        None => {
            let mut child = host.first_child();
            while let Some(widget) = child.as_ref().filter(|widget| !is_closing_layer(widget)) {
                child = widget.next_sibling();
            }
            child
        }
    };
    match closing {
        Some(closing) => layer_focus(&closing).opener.borrow().clone(),
        None => focus.map(|focus| focus.downgrade()),
    }
}

/// Tracks the opener of a layer that is added and removed, and returns focus to it
/// when the layer closes over another modal.
fn track_removed_layer(layer: &gtk::Widget) {
    let focus = layer_focus(layer);
    layer.connect_parent_notify(move |layer| {
        if let Some(parent) = layer.parent() {
            let opener = parent
                .root()
                .and_downcast::<gtk::Window>()
                .and_then(|window| layer_opener(&parent, &window));
            focus.opener.replace(opener);
            focus.host.replace(Some(parent.downgrade()));
            return;
        }
        if !layer.has_css_class("dismissing") {
            return;
        }
        let Some(window) = focus
            .host
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
            .and_then(|host| host.root())
            .and_downcast::<gtk::Window>()
        else {
            return;
        };
        if let Some(under) = crate::ui::window::visible_modal_layer(&window) {
            restore_focus_under(&window, &under, &focus);
        }
    });
}


fn modal_focus_fallback(window: &gtk::Window) -> Option<Rc<dyn Fn()>> {
    MODAL_FOCUS_FALLBACKS.with(|fallbacks| {
        let mut fallbacks = fallbacks.borrow_mut();
        fallbacks.retain(|(candidate, _)| candidate.upgrade().is_some());
        fallbacks
            .iter()
            .find(|(candidate, _)| {
                candidate
                    .upgrade()
                    .is_some_and(|candidate| candidate == *window)
            })
            .map(|(_, fallback)| fallback.clone())
    })
}

/// Records the window focus for `layer`, or the browser origin of the modal it is chained on.
fn register_focus_origin(
    layer: &gtk::Widget,
    window: Option<&gtk::Window>,
    allow_widget: bool,
) -> Rc<ModalFocusOrigin> {
    let previous = window.and_then(crate::ui::window::visible_modal_layer);
    MODAL_FOCUS_ORIGINS.with(|origins| {
        let mut origins = origins.borrow_mut();
        origins.retain(|(layer, _)| layer.upgrade().is_some());
        // Chained dialogs inherit the browser origin, not the preceding modal's focus.
        let origin = previous
            .and_then(|previous| {
                origins.iter().find_map(|(layer, origin)| {
                    layer
                        .upgrade()
                        .filter(|layer| *layer == previous)
                        .map(|_| origin.clone())
                })
            })
            .unwrap_or_else(|| {
                Rc::new(ModalFocusOrigin {
                    widget: window
                        .and_then(gtk::prelude::RootExt::focus)
                        .map(|focus| focus.downgrade()),
                    restore: RefCell::new(None),
                    allow_widget,
                })
            });
        origins.push((layer.downgrade(), origin.clone()));
        origin
    })
}

fn forget_focus_origin(layer: &gtk::Widget) {
    MODAL_FOCUS_ORIGINS.with(|origins| {
        origins.borrow_mut().retain(|(candidate, _)| {
            candidate
                .upgrade()
                .is_some_and(|candidate| candidate != *layer)
        });
    });
}

/// Priority: focus taken meanwhile, explicit restore, on-screen origin, window fallback.
fn restore_modal_focus(window: &gtk::Window, origin: &ModalFocusOrigin, allow_origin: bool) {
    if gtk::prelude::RootExt::focus(window).is_some_and(|focus| focus.is_mapped()) {
        return;
    }
    let restore = origin.restore.borrow().clone();
    if let Some(restore) = restore {
        restore();
        return;
    }
    if allow_origin
        && origin.allow_widget
        && let Some(widget) = origin.widget.as_ref().and_then(glib::WeakRef::upgrade)
        && shown_or_revealing(&widget)
        && widget.is_sensitive()
        && widget.root().as_ref() == Some(window.upcast_ref())
        && widget.grab_focus()
    {
        return;
    }
    if let Some(fallback) = modal_focus_fallback(window) {
        fallback();
    }
}

/// Whether `widget` is on screen or is mapped by the next frame: an opening revealer,
/// such as a just-shown filter field's, maps its child only on its first animation
/// tick, which a slow paint can delay until after an overlay opened over it closes.
fn shown_or_revealing(widget: &gtk::Widget) -> bool {
    let mut current = widget.clone();
    while !current.is_mapped() {
        let Some(parent) = current.parent() else {
            return false;
        };
        let revealing = parent
            .downcast_ref::<gtk::Revealer>()
            .is_some_and(gtk::Revealer::reveals_child);
        if !current.is_visible() || !(current.is_child_visible() || revealing) {
            return false;
        }
        current = parent;
    }
    true
}

pub(super) fn remember_modal_focus(layer: &gtk::Box, overlay: &gtk::Overlay) -> Rc<Cell<bool>> {
    remember_removed_modal_focus(layer, overlay, true)
}


fn remember_removed_modal_focus(
    layer: &gtk::Box,
    overlay: &gtk::Overlay,
    allow_origin: bool,
) -> Rc<Cell<bool>> {
    let window = overlay.root().and_downcast::<gtk::Window>();
    let origin = register_focus_origin(layer.upcast_ref(), window.as_ref(), allow_origin);
    let overlay = overlay.downgrade();
    let restore = Rc::new(Cell::new(true));
    let restore_on_close = restore.clone();
    layer.connect_parent_notify(move |layer| {
        if layer.parent().is_some() {
            return;
        }
        forget_focus_origin(layer.upcast_ref());
        if !layer.has_css_class("dismissing") || !restore_on_close.get() {
            return;
        }
        // The focus trap releases the browser only after the final modal is removed.
        let Some(window) = overlay
            .upgrade()
            .and_then(|overlay| overlay.root())
            .and_downcast::<gtk::Window>()
        else {
            return;
        };
        if crate::ui::window::visible_modal_layer(&window).is_some() {
            return;
        }
        restore_modal_focus(&window, &origin, allow_origin);
        rearm_focus_ring(&window, &layer_focus(layer.upcast_ref()));
    });
    restore
}



pub(super) struct ModalHost {
    pub(super) overlay: gtk::Overlay,
    pub(super) blurred_root: Option<BlurBin>,
}

impl ModalHost {
    pub(super) fn for_widget(parent: &impl IsA<gtk::Widget>) -> Option<Self> {
        let overlay = window_overlay(parent)?;
        let blurred_root = overlay.child().and_downcast::<BlurBin>();
        Some(Self {
            overlay,
            blurred_root,
        })
    }

    pub(super) fn blurred_for(parent: &impl IsA<gtk::Widget>) -> Option<Self> {
        let host = Self::for_widget(parent)?;
        if let Some(root) = host.blurred_root.as_ref() {
            root.set_blurred(true);
        }
        Some(host)
    }
}

pub(super) fn window_overlay(parent: &impl IsA<gtk::Widget>) -> Option<gtk::Overlay> {
    parent
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| window.child())
        .and_downcast::<gtk::Overlay>()
}

pub(super) fn modal_layer(
    content: &impl IsA<gtk::Widget>,
    overlay: &gtk::Overlay,
    root: Option<BlurBin>,
    block_dismiss: Option<Rc<dyn Fn() -> bool>>,
) -> gtk::Box {
    let overlay = overlay.clone();
    modal_layer_with_backdrop(
        content,
        Rc::new(move |layer: &gtk::Box| {
            if block_dismiss.as_ref().is_some_and(|block| block()) {
                return;
            }
            dismiss_modal_layer(layer, &overlay, root.as_ref());
        }),
    )
}

#[expect(
    deprecated,
    reason = "GTK 4.12 deprecated translate_coordinates and allocation without a replacement for click-in-bounds checks"
)]
pub(super) fn modal_layer_with_backdrop(
    content: &impl IsA<gtk::Widget>,
    on_backdrop: Rc<dyn Fn(&gtk::Box)>,
) -> gtk::Box {
    let layer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layer.add_css_class("app-modal-layer");
    layer.add_css_class("modal-backdrop");
    layer.set_halign(gtk::Align::Fill);
    layer.set_valign(gtk::Align::Fill);
    layer.set_hexpand(true);
    layer.set_vexpand(true);
    layer.set_focusable(true);
    let viewport = layout::install(&layer, content);

    let click = gtk::GestureClick::new();
    let weak_layer = layer.downgrade();
    let weak_viewport = viewport.downgrade();
    let weak_content = content.as_ref().downgrade();
    click.connect_pressed(move |_, _, x, y| {
        let Some(layer) = weak_layer.upgrade() else {
            return;
        };
        let Some(content) = weak_content.upgrade() else {
            return;
        };
        let Some(viewport) = weak_viewport.upgrade() else {
            return;
        };
        let on_dialog = [content, viewport.upcast()].iter().all(|widget| {
            widget
                .translate_coordinates(&layer, 0.0, 0.0)
                .is_some_and(|(cx, cy)| {
                    let alloc = widget.allocation();
                    x >= cx
                        && x < cx + alloc.width() as f64
                        && y >= cy
                        && y < cy + alloc.height() as f64
                })
        });
        if !on_dialog {
            on_backdrop(&layer);
        }
    });
    layer.add_controller(click);
    crate::ui::focus_navigation::install(&layer);
    track_removed_layer(layer.upcast_ref());
    animate_in(&layer);
    layer
}

pub(super) fn close_on_escape(
    widget: &impl IsA<gtk::Widget>,
    close: impl Fn() + 'static,
) -> gtk::EventControllerKey {
    let escape = gtk::EventControllerKey::new();
    keys::on_press(&escape, move |_, press| {
        if !press.is(ESCAPE_ANY) {
            return glib::Propagation::Proceed;
        }
        close();
        glib::Propagation::Stop
    });
    widget.add_controller(escape.clone());
    escape
}

pub(super) fn submit_on_enter(fields: &impl IsA<gtk::Widget>, confirm: &gtk::Button) {
    let mut child = fields.as_ref().first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
            let confirm = confirm.clone();
            entry.connect_activate(move |_| activate_primary(&confirm));
        } else if let Some(entry) = widget.downcast_ref::<gtk::PasswordEntry>() {
            let confirm = confirm.clone();
            entry.connect_activate(move |_| activate_primary(&confirm));
        } else {
            submit_on_enter(&widget, confirm);
        }
    }
}

fn activate_primary(confirm: &gtk::Button) {
    if confirm.is_sensitive() {
        confirm.emit_clicked();
    }
}

pub(super) fn animate_in(layer: &gtk::Box) {
    layer.remove_css_class("dismissing");
    layer.set_sensitive(true);
    layer.add_css_class("modal-hidden");
    let weak = layer.downgrade();
    glib::timeout_add_local_once(Duration::from_millis(16), move || {
        if let Some(layer) = weak.upgrade() {
            layer.remove_css_class("modal-hidden");
        }
    });
}

pub(super) fn animate_out(layer: &gtk::Box, on_done: impl FnOnce() + 'static) {
    layer.add_css_class("modal-hidden");
    glib::timeout_add_local_once(Duration::from_millis(200), on_done);
}

pub(super) fn slide_out(widget: &impl IsA<gtk::Widget>) {
    let w = widget.as_ref();
    w.remove_css_class("slide-out");
    w.add_css_class("slide-out");
    let weak = w.downgrade();
    glib::timeout_add_local_once(Duration::from_millis(240), move || {
        if let Some(w) = weak.upgrade() {
            w.remove_css_class("slide-out");
        }
    });
}


pub(super) fn dismiss_modal_layer(
    layer: &gtk::Box,
    overlay: &gtk::Overlay,
    root: Option<&BlurBin>,
) {
    dismiss_modal_layer_then(layer, overlay, root, || {});
}

pub(super) fn dismiss_modal_layer_then(
    layer: &gtk::Box,
    overlay: &gtk::Overlay,
    root: Option<&BlurBin>,
    on_done: impl FnOnce() + 'static,
) {
    if layer.has_css_class("dismissing") {
        return;
    }
    layer.add_css_class("dismissing");
    layer.set_sensitive(false);
    let overlay = overlay.clone();
    let layer_for_anim = layer.clone();
    let layer = layer.clone();
    let root = root.cloned();
    animate_out(&layer_for_anim, move || {
        overlay.remove_overlay(&layer);
        if let Some(root) = root
            && !overlay_has_modal_layer(&overlay)
        {
            root.set_blurred(false);
        }
        on_done();
    });
}

fn overlay_has_modal_layer(overlay: &gtk::Overlay) -> bool {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        if widget.is_visible() && widget.has_css_class("app-modal-layer") {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Error,
    PartialFailure,
    Information,
}

pub(super) fn show_error_dialog(parent: &impl IsA<gtk::Widget>, message: &str, detail: &str) {
    show_error_dialog_after_close(parent, message, detail, Rc::new(|| {}));
}


pub(super) fn show_information_dialog(parent: &impl IsA<gtk::Widget>, message: &str, detail: &str) {
    show_message_dialog(
        parent,
        message,
        None,
        detail,
        MessageKind::Information,
        Rc::new(|| {}),
    );
}


pub(super) fn show_error_dialog_after_close(
    parent: &impl IsA<gtk::Widget>,
    message: &str,
    detail: &str,
    on_close: Rc<dyn Fn()>,
) {
    show_message_dialog(parent, message, None, detail, MessageKind::Error, on_close);
}

/// [`show_error_dialog_after_close`] with `summary` under the title in place of
/// the generic "The operation could not be completed".
pub(super) fn show_error_dialog_with_summary(
    parent: &impl IsA<gtk::Widget>,
    message: &str,
    summary: &str,
    detail: &str,
    on_close: Rc<dyn Fn()>,
) {
    show_message_dialog(
        parent,
        message,
        Some(summary),
        detail,
        MessageKind::Error,
        on_close,
    );
}

/// Reports a message as a toast over the window, the 2.0 way: the heading, with
/// the detail inline when it is short, or behind a Details action that opens it
/// in a dialog. Without a window to toast over, the dialog opens directly.
fn show_message_dialog(
    parent: &impl IsA<gtk::Widget>,
    message: &str,
    summary: Option<&str>,
    detail: &str,
    kind: MessageKind,
    on_close: Rc<dyn Fn()>,
) {
    let parent = parent.as_ref();
    let overlay = window_overlay(parent).or_else(|| {
        parent
            .clone()
            .downcast::<gtk::Overlay>()
            .ok()
            .or_else(|| parent.ancestor(gtk::Overlay::static_type())?.downcast().ok())
    });
    let window = parent.root().and_downcast::<gtk::Window>();
    let detail = detail.trim().to_owned();
    let summary = summary.map(str::to_owned).or_else(|| {
        (kind == MessageKind::PartialFailure)
            .then(|| crate::i18n::tr("Some items could not be processed"))
    });
    let inline = summary.is_none() && !detail.contains('\n') && detail.chars().count() <= 90;
    match overlay {
        Some(overlay) if inline => {
            let text = if detail.is_empty() {
                message.to_owned()
            } else {
                format!("{message}: {detail}")
            };
            nyx_ui::toast::show(&overlay, &text);
        }
        Some(overlay) => {
            let message_for_details = message.to_owned();
            nyx_ui::toast::show_action(&overlay, message, &crate::i18n::tr("Details"), move || {
                details_dialog(
                    window.as_ref(),
                    &message_for_details,
                    summary.as_deref(),
                    &detail,
                );
            });
        }
        None => details_dialog(window.as_ref(), message, summary.as_deref(), &detail),
    }
    on_close();
}

/// A message with its full, selectable detail in a 2.0 dialog.
fn details_dialog(parent: Option<&gtk::Window>, message: &str, summary: Option<&str>, detail: &str) {
    let (dialog, card) = nyx_ui::widgets::dialog(&dialog_text(message), 480);
    if let Some(parent) = parent {
        dialog.set_transient_for(Some(parent));
    }
    if let Some(summary) = summary {
        let summary = nyx_ui::widgets::label(&dialog_text(summary), "dim");
        summary.set_wrap(true);
        card.append(&summary);
    }
    if !detail.is_empty() {
        let explanation = message_dialog_description(detail);
        explanation.set_selectable(true);
        card.append(&explanation);
    }
    let close = gtk::Button::with_label(&crate::i18n::tr("Close"));
    close.add_css_class("suggested-action");
    close.set_halign(gtk::Align::End);
    close.set_margin_top(6);
    let closing = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = closing.upgrade() {
            dialog.close();
        }
    });
    card.append(&close);
    dialog.present();
    close.grab_focus();
}

/// A 2.0 confirmation over `parent`'s window: the question, what it does, then
/// Cancel and the action (in the danger colour when `danger`). `on_confirm`
/// runs after the dialog closes; `on_close` runs however it closes.
pub(super) fn confirm(
    parent: &impl IsA<gtk::Widget>,
    question: &str,
    details: &[&str],
    action: &str,
    danger: bool,
    on_confirm: impl Fn() + 'static,
    on_close: Option<Rc<dyn Fn()>>,
) {
    let (dialog, card) = nyx_ui::widgets::dialog(&dialog_text(question), 420);
    if let Some(window) = parent.as_ref().root().and_downcast::<gtk::Window>() {
        dialog.set_transient_for(Some(&window));
    }
    for detail in details.iter().filter(|detail| !detail.is_empty()) {
        card.append(&message_dialog_description(detail));
    }
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    let confirm = gtk::Button::with_label(action);
    confirm.add_css_class(if danger {
        "destructive-action"
    } else {
        "suggested-action"
    });
    buttons.append(&cancel);
    buttons.append(&confirm);
    card.append(&buttons);
    let closing = dialog.downgrade();
    cancel.connect_clicked(move |_| {
        if let Some(dialog) = closing.upgrade() {
            dialog.close();
        }
    });
    let closing = dialog.downgrade();
    confirm.connect_clicked(move |_| {
        if let Some(dialog) = closing.upgrade() {
            dialog.close();
        }
        on_confirm();
    });
    if let Some(on_close) = on_close {
        dialog.connect_close_request(move |_| {
            let on_close = on_close.clone();
            glib::idle_add_local_once(move || on_close());
            glib::Propagation::Proceed
        });
    }
    dialog.present();
    confirm.grab_focus();
}
