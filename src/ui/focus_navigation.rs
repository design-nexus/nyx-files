// SPDX-License-Identifier: MIT

use gtk::{gdk, glib, prelude::*};

use super::keys::{
    self, Binding, KeyPress,
    bindings::{
        DOWN, ENTER, FOCUS_NEXT, FOCUS_PREVIOUS, LEFT, RIGHT, UP, VIM_DOWN, VIM_LEFT, VIM_RIGHT,
        VIM_UP,
    },
};

#[cfg(test)]
mod tests;






pub(super) fn arrow_direction(
    press: &KeyPress,
    tolerated: gdk::ModifierType,
) -> Option<gtk::DirectionType> {
    [
        (LEFT, gtk::DirectionType::Left),
        (RIGHT, gtk::DirectionType::Right),
        (UP, gtk::DirectionType::Up),
        (DOWN, gtk::DirectionType::Down),
    ]
    .into_iter()
    .find(|(arrow, _)| press.is(arrow.ignoring(tolerated)))
    .map(|(_, direction)| direction)
}






pub(super) fn editable(widget: &gtk::Widget) -> bool {
    widget.is::<gtk::Editable>()
        || widget.is::<gtk::TextView>()
        || widget.ancestor(gtk::Entry::static_type()).is_some()
        || widget.ancestor(gtk::TextView::static_type()).is_some()
}

/// Unlike `editable`, a read-only text view is a document, not a field.
pub(super) fn accepts_typing(widget: &gtk::Widget) -> bool {
    let view = widget.downcast_ref::<gtk::TextView>().cloned().or_else(|| {
        widget
            .ancestor(gtk::TextView::static_type())
            .and_downcast::<gtk::TextView>()
    });
    match view {
        Some(view) => view.is_editable(),
        None => editable(widget),
    }
}

pub(super) fn in_popover(widget: &gtk::Widget) -> bool {
    widget.is::<gtk::Popover>() || widget.ancestor(gtk::Popover::static_type()).is_some()
}

fn controls(scope: &gtk::Widget, result: &mut Vec<gtk::Widget>) {
    if !scope.is_mapped() || !scope.is_sensitive() {
        return;
    }
    if scope.is::<gtk::Popover>()
        || scope.is::<gtk::ListView>()
        || scope.is::<gtk::GridView>()
        || scope.is::<gtk::TextView>()
    {
        return;
    }
    if let Some(tab) = scope.downcast_ref::<gtk::Notebook>().and_then(current_tab) {
        result.push(tab);
    }
    if scope.is::<gtk::Button>()
        || scope.is::<gtk::MenuButton>()
        || scope.is::<gtk::CheckButton>()
        || scope.is::<gtk::Switch>()
        || scope.is::<gtk::Entry>()
    {
        result.push(scope.clone());
        return;
    }
    let mut child = scope.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        controls(&widget, result);
    }
}

/// A notebook's current tab stands in for the notebook, which holds focus while a
/// tab does.
fn current_tab(notebook: &gtk::Notebook) -> Option<gtk::Widget> {
    if !notebook.shows_tabs() {
        return None;
    }
    let page = notebook.nth_page(notebook.current_page())?;
    notebook
        .tab_label(&page)?
        .parent()
        .filter(gtk::Widget::is_mapped)
}

fn tab_owner(widget: &gtk::Widget) -> Option<gtk::Notebook> {
    widget
        .ancestor(gtk::Notebook::static_type())
        .and_downcast::<gtk::Notebook>()
        .filter(|notebook| current_tab(notebook).as_ref() == Some(widget))
}

fn focused_control(scope: &gtk::Widget) -> Option<(Vec<gtk::Widget>, usize)> {
    let focused = scope.root()?.focus()?;
    if editable(&focused) || in_popover(&focused) {
        return None;
    }
    let focused = match focused.downcast_ref::<gtk::Notebook>() {
        Some(notebook) => current_tab(notebook)?,
        None => focused,
    };
    let mut candidates = Vec::new();
    controls(scope, &mut candidates);
    let index = candidates
        .iter()
        .position(|candidate| *candidate == focused || focused.is_ancestor(candidate))?;
    Some((candidates, index))
}

pub(super) fn activate(scope: &gtk::Widget) -> bool {
    let Some((controls, current)) = focused_control(scope) else {
        return false;
    };
    let widget = &controls[current];
    if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
        button.popup();
    } else if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        button.emit_clicked();
    } else if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
        check.set_active(!check.is_active());
    } else if let Some(switch) = widget.downcast_ref::<gtk::Switch>() {
        switch.set_active(!switch.is_active());
    } else {
        return false;
    }
    true
}

pub(super) fn move_focus(scope: &gtk::Widget, direction: gtk::DirectionType) -> bool {
    let Some((controls, current)) = focused_control(scope) else {
        return false;
    };
    // GTK's own Left and Right move between tabs.
    if matches!(
        direction,
        gtk::DirectionType::Left | gtk::DirectionType::Right
    ) && tab_owner(&controls[current]).is_some()
    {
        return false;
    }
    if direction == gtk::DirectionType::Down
        && let Some(button) = controls[current].downcast_ref::<gtk::MenuButton>()
    {
        button.popup();
        return true;
    }
    let bounds = controls
        .iter()
        .map(|widget| widget.compute_bounds(scope))
        .collect::<Vec<_>>();
    let Some(origin) = bounds[current].as_ref() else {
        return false;
    };
    let vertical = matches!(direction, gtk::DirectionType::Up | gtk::DirectionType::Down);
    let target = bounds
        .iter()
        .enumerate()
        .filter_map(|(index, bounds)| {
            if index == current {
                return None;
            }
            let mut bounds = *bounds.as_ref()?;
            // The tab strip runs across the whole notebook, so Up and Down treat it as
            // directly above or below the control they leave.
            if vertical && tab_owner(&controls[index]).is_some() {
                bounds = gtk::graphene::Rect::new(
                    origin.x(),
                    bounds.y(),
                    origin.width(),
                    bounds.height(),
                );
            }
            directional_distance(origin, &bounds, direction).map(|score| (index, score))
        })
        .min_by(|(_, a), (_, b)| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    target.is_some_and(|(index, _)| {
        let target = &controls[index];
        let moved = match tab_owner(target) {
            Some(notebook) => notebook.grab_focus(),
            None => target.grab_focus() || target.child_focus(gtk::DirectionType::TabForward),
        };
        if moved && let Some(window) = scope.root().and_downcast::<gtk::Window>() {
            window.set_focus_visible(true);
        }
        moved
    })
}

fn directional_distance(
    origin: &gtk::graphene::Rect,
    target: &gtk::graphene::Rect,
    direction: gtk::DirectionType,
) -> Option<(bool, f32)> {
    let horizontal = matches!(
        direction,
        gtk::DirectionType::Left | gtk::DirectionType::Right
    );
    let (along, across, origin_size, target_size) = if horizontal {
        (
            target.center().x() - origin.center().x(),
            target.center().y() - origin.center().y(),
            origin.height(),
            target.height(),
        )
    } else {
        (
            target.center().y() - origin.center().y(),
            target.center().x() - origin.center().x(),
            origin.width(),
            target.width(),
        )
    };
    let sign = if matches!(direction, gtk::DirectionType::Left | gtk::DirectionType::Up) {
        -1.0
    } else {
        1.0
    };
    let distance = along * sign;
    (distance > 1.0).then_some((
        across.abs() * 2.0 > origin_size + target_size,
        distance + across.abs() * 2.0,
    ))
}

pub(super) fn install(scope: &impl IsA<gtk::Widget>) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = scope.as_ref().downgrade();
    keys::on_press(&keys, move |_, press| {
        let Some(scope) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let handled = if let Some(direction) = arrow_direction(press, gdk::ModifierType::empty()) {
            move_focus(&scope, direction)
        } else if press.is(ENTER) {
            activate(&scope)
        } else {
            false
        };
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    scope.add_controller(keys);
}
