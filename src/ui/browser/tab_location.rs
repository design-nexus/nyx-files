// SPDX-License-Identifier: MIT

use std::rc::Rc;

use crate::model::Location;

use super::{BrowserView, ViewState};

type Observer = Rc<dyn Fn(Option<&Location>)>;

#[derive(Default)]
pub(super) struct TabLocation {
    location: Option<Location>,
    observers: Vec<Observer>,
    next_id: u64,
    pending: Option<Pending>,
}

struct Pending {
    id: u64,
    phase: Phase,
    /// The folder a click is opening, which becomes the focused column.
    opening: Option<(usize, Location)>,
}

enum Phase {
    Pressed,
    Dispatching,
    Awaiting {
        generation: u64,
        parent: Option<(usize, Location)>,
    },
}




impl BrowserView {
}

impl ViewState {

    pub(super) fn pointer_opening_depth(&self) -> Option<usize> {
        self.tab_location
            .borrow()
            .pending
            .as_ref()
            .and_then(|pending| pending.opening.as_ref())
            .map(|(depth, _)| *depth)
    }

    pub(super) fn pointer_opens(&self, depth: usize, location: &Location) -> bool {
        self.tab_location
            .borrow()
            .pending
            .as_ref()
            .and_then(|pending| pending.opening.as_ref())
            .is_some_and(|(opening_depth, opening)| *opening_depth == depth && opening == location)
    }

    pub(super) fn cancel_tab_location_hold(&self) {
        self.tab_location.borrow_mut().pending = None;
        self.publish_tab_location();
    }


    fn publish_tab_location(&self) {
        if self.browser.navigation_update_in_progress() {
            return;
        }
        let current = self.browser.active_location();
        let observers = {
            let mut location = self.tab_location.borrow_mut();
            if location.pending.is_some() || location.location == current {
                return;
            }
            location.location = current.clone();
            location.observers.clone()
        };
        for observer in observers {
            observer(current.as_ref());
        }
    }
}



#[cfg(test)]
mod tests;
