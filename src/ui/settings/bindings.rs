// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::theme::ThemeManager;




pub(super) fn bind_switch(
    manager: &Rc<PreferenceManager>,
    toggle: &gtk::Switch,
    read: fn(&PreferenceManager) -> bool,
    write: fn(&PreferenceManager, bool),
) {
    manager.bind_preference(toggle, read, |widget, value| {
        if let Some(toggle) = widget.downcast_ref::<gtk::Switch>() {
            toggle.set_active(value);
        }
    });
    let manager = manager.clone();
    toggle.connect_active_notify(move |toggle| {
        let value = toggle.is_active();
        if read(&manager) != value {
            write(&manager, value);
        }
    });
}



