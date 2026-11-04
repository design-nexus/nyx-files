// SPDX-License-Identifier: MIT

//! An Appearance menu density slider: any px value in range, with clickable
//! Compact and Airy labels under their tick marks that snap to those presets.

use std::{cell::RefCell, rc::Rc};

use gtk::{glib, graphene, gsk, prelude::*, subclass::prelude::*};

use crate::ui::{browser_modes::DensityScale, preferences::PreferenceManager};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct SnapLabels {
        pub(super) scale: glib::WeakRef<gtk::Scale>,
        pub(super) labels: RefCell<Vec<(u8, gtk::Button)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SnapLabels {
        const NAME: &'static str = "StrataDensitySnapLabels";
        type Type = super::SnapLabels;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SnapLabels {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for SnapLabels {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let (mut minimum, mut natural) = (0, 0);
            for (_, label) in self.labels.borrow().iter() {
                let (child_minimum, child_natural, _, _) = label.measure(orientation, -1);
                minimum = minimum.max(child_minimum);
                natural = if orientation == gtk::Orientation::Horizontal {
                    natural + child_natural
                } else {
                    natural.max(child_natural)
                };
            }
            (minimum, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let centers = self.mark_centers();
            for (index, (px, label)) in self.labels.borrow().iter().enumerate() {
                let label_width = label.measure(gtk::Orientation::Horizontal, -1).1.min(width);
                let center = centers
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| self.fallback_center(*px, width));
                let x =
                    (center - label_width as f32 / 2.0).clamp(0.0, (width - label_width) as f32);
                label.allocate(
                    label_width,
                    height,
                    -1,
                    Some(gsk::Transform::new().translate(&graphene::Point::new(x, 0.0))),
                );
            }
        }
    }

    impl SnapLabels {
        /// GTK places each `mark` node (a documented scale CSS node) centred on its
        /// value; marks are added in ascending order, matching `labels`.
        fn mark_centers(&self) -> Vec<f32> {
            let Some(scale) = self.scale.upgrade() else {
                return Vec::new();
            };
            let obj = self.obj();
            let mut centers = Vec::new();
            let mut pending = vec![scale.upcast::<gtk::Widget>()];
            while let Some(widget) = pending.pop() {
                if widget.css_name() == "mark" {
                    if let Some(bounds) = widget.compute_bounds(&*obj) {
                        centers.push(bounds.x() + bounds.width() / 2.0);
                    }
                    continue;
                }
                let mut child = widget.last_child();
                while let Some(current) = child {
                    child = current.prev_sibling();
                    pending.push(current);
                }
            }
            if centers.len() == self.labels.borrow().len() {
                centers.sort_by(f32::total_cmp);
                centers
            } else {
                Vec::new()
            }
        }

        fn fallback_center(&self, px: u8, width: i32) -> f32 {
            let max = self
                .scale
                .upgrade()
                .map_or(1.0, |scale| scale.adjustment().upper().max(1.0));
            (f64::from(px) / max * f64::from(width)) as f32
        }
    }
}

glib::wrapper! {
    pub struct SnapLabels(ObjectSubclass<imp::SnapLabels>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SnapLabels {
}



