// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn toasts_leave_the_progress_card_in_place() {
    crate::test_support::gtk_test(
        "interface::operations::tests::toasts_leave_the_progress_card_in_place",
        || {
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
            let progress = Progress::new(&overlay);
            toast::show(&overlay, "Copied");
            toast::show_undo(&overlay, "Moved to Trash", || {});
            toast::dismiss(&overlay);
            assert_eq!(
                progress.holder.parent().as_ref(),
                Some(overlay.upcast_ref::<gtk::Widget>())
            );
        },
    );
}
