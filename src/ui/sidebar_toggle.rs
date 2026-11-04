// SPDX-License-Identifier: MIT

//! The header's sidebar button. A click steps the sidebar through its modes
//! instead of flipping the active state, so one control reaches the full
//! sidebar, the icon rail, and the hidden sidebar.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, prelude::*, subclass::prelude::*};

use crate::{
    assets::{self, icons},
    ui::keys::{
        Keys,
        bindings::{SIDEBAR, TENXER_SIDEBAR},
    },
};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SidebarToggle {
        pub(super) step: RefCell<Option<Rc<dyn Fn()>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SidebarToggle {
        const NAME: &'static str = "StrataSidebarToggle";
        type Type = super::SidebarToggle;
        type ParentType = gtk::ToggleButton;
    }

    impl ObjectImpl for SidebarToggle {}
    impl WidgetImpl for SidebarToggle {}

    impl ButtonImpl for SidebarToggle {
        fn clicked(&self) {
            let step = self.step.borrow().clone();
            match step {
                Some(step) => step(),
                None => self.parent_clicked(),
            }
        }
    }

    impl ToggleButtonImpl for SidebarToggle {}
}

glib::wrapper! {
    pub struct SidebarToggle(ObjectSubclass<imp::SidebarToggle>)
        @extends gtk::ToggleButton, gtk::Button, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}



impl SidebarToggle {


}


