// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*};

pub(super) fn exceeds_drag_threshold(press: (f64, f64), point: (f64, f64), threshold: i32) -> bool {
    (point.0 - press.0).abs() > f64::from(threshold)
        || (point.1 - press.1).abs() > f64::from(threshold)
}






struct PendingClick {
    item: glib::Object,
    position: u32,
    press: (f64, f64),
    moved: bool,
}

/// A recycled row or a gesture that crossed the drag threshold cannot activate on
/// release, even if the pointer returned to its original position or DnD was cancelled.
pub(super) fn connect_click_release(
    click: &gtk::GestureClick,
    item: &gtk::ListItem,
    released: impl Fn(&gtk::GestureClick, i32, f64, f64) + 'static,
) {
    let pending = Rc::new(RefCell::new(None::<PendingClick>));
    let item_for_press = item.downgrade();
    let pending_for_press = pending.clone();
    click.connect_pressed(move |_, _, x, y| {
        pending_for_press.take();
        if let Some(item) = item_for_press.upgrade()
            && item.position() != gtk::INVALID_LIST_POSITION
            && let Some(value) = item.item()
        {
            pending_for_press.replace(Some(PendingClick {
                item: value,
                position: item.position(),
                press: (x, y),
                moved: false,
            }));
        }
    });
    let pending_for_update = pending.clone();
    click.connect_update(move |gesture, sequence| {
        if let (Some(pending), Some(point), Some(widget)) = (
            pending_for_update.borrow_mut().as_mut(),
            gesture.point(sequence),
            gesture.widget(),
        ) {
            pending.moved |= exceeds_drag_threshold(
                pending.press,
                point,
                widget.settings().gtk_dnd_drag_threshold(),
            );
        }
    });
    let pending_for_cancel = pending.clone();
    click.connect_cancel(move |_, _| {
        pending_for_cancel.take();
    });
    let item = item.downgrade();
    click.connect_released(move |gesture, count, x, y| {
        let (Some(pending), Some(item), Some(widget)) =
            (pending.take(), item.upgrade(), gesture.widget())
        else {
            return;
        };
        if !pending.moved
            && !exceeds_drag_threshold(
                pending.press,
                (x, y),
                widget.settings().gtk_dnd_drag_threshold(),
            )
            && item.position() == pending.position
            && item.item().as_ref() == Some(&pending.item)
        {
            released(gesture, count, pending.press.0, pending.press.1);
        }
    });
}

#[cfg(test)]
mod tests;
