// SPDX-License-Identifier: GPL-3.0-or-later
use crate::model::FileEntry;
use crate::ui::modal::window_overlay;
use gtk::glib;
use gtk::gsk::prelude::IsRenderNode;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};
use std::time::Duration;


pub(super) struct PreparedDissolve {
    overlay: gtk::Overlay,
    canvas: DissolveCanvas,
    source: glib::WeakRef<gtk::Widget>,
    source_presentation: Option<(f64, bool)>,
}

impl Drop for PreparedDissolve {
    fn drop(&mut self) {
        if self.canvas.parent().is_some() {
            self.overlay.remove_overlay(&self.canvas);
        }
        if let Some((opacity, can_target)) = self.source_presentation
            && let Some(source) = self.source.upgrade()
        {
            source.set_opacity(opacity);
            source.set_can_target(can_target);
        }
    }
}

#[derive(Clone)]
struct FragmentMotion {
    source: gtk::graphene::Rect,
    offset: (f64, f64),
    delay: f64,
}

#[derive(Clone)]
struct Fragment {
    node: gtk::gsk::RenderNode,
    motion: FragmentMotion,
}

#[derive(Clone)]
struct DissolveRow {
    origin: gtk::graphene::Point,
    fragments: Vec<Fragment>,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub(super) struct DissolveCanvas {
        pub(super) rows: RefCell<Vec<DissolveRow>>,
        pub(super) backdrop: RefCell<Option<gtk::gsk::RenderNode>>,
        pub(super) progress: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DissolveCanvas {
        const NAME: &'static str = "StrataDissolveCanvas";
        type Type = super::DissolveCanvas;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for DissolveCanvas {}

    impl WidgetImpl for DissolveCanvas {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(backdrop) = self.backdrop.borrow().as_ref() {
                snapshot.append_node(backdrop);
            }
            let progress = self.progress.get();
            for row in self.rows.borrow().iter() {
                for fragment in &row.fragments {
                    snapshot_fragment(snapshot, row, fragment, progress);
                }
            }
        }
    }
}

glib::wrapper! {
    struct DissolveCanvas(ObjectSubclass<imp::DissolveCanvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DissolveCanvas {

}


impl PreparedDissolve {
}



fn snapshot_fragment(
    snapshot: &gtk::Snapshot,
    row: &DissolveRow,
    fragment: &Fragment,
    progress: f64,
) {
    let motion = &fragment.motion;
    let local = ((progress - motion.delay) / (1.0 - motion.delay)).clamp(0.0, 1.0);
    let movement = ease_out_cubic(local);
    let opacity = 1.0 - ease_in_cubic(local);
    if opacity <= 0.0 {
        return;
    }

    let lift = motion.offset.1 * movement + 14.0 * local * local;
    snapshot.save();
    snapshot.translate(&gtk::graphene::Point::new(
        row.origin.x() + (motion.offset.0 * movement) as f32,
        row.origin.y() + lift as f32,
    ));
    snapshot.push_opacity(opacity);
    snapshot.append_node(&fragment.node);
    snapshot.pop();
    snapshot.restore();
}





fn ease_in_cubic(progress: f64) -> f64 {
    progress * progress * progress
}

fn ease_out_cubic(progress: f64) -> f64 {
    1.0 - (1.0 - progress).powi(3)
}


