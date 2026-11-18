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

#[test]
fn progress_card_keeps_fixed_size_across_text_updates() {
    crate::test_support::gtk_test(
        "interface::operations::tests::progress_card_keeps_fixed_size_across_text_updates",
        || {
            let overlay = gtk::Overlay::new();
            overlay.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
            let progress = Progress::new(&overlay);
            let card = progress
                .holder
                .first_child()
                .expect("progress card inside holder");

            let measure = || {
                let (min_w, nat_w, _, _) = card.measure(gtk::Orientation::Horizontal, -1);
                let (min_h, nat_h, _, _) = card.measure(gtk::Orientation::Vertical, nat_w);
                (min_w, nat_w, min_h, nat_h)
            };

            let baseline = measure();
            assert_eq!(baseline.0, 320);
            assert_eq!(baseline.1, 320);

            progress.title.set_text("Copying · 1 item");
            progress.detail.set_text("a.txt");
            assert_eq!(measure(), baseline);

            progress
                .title
                .set_text("Downloading a very long archive title that would otherwise stretch");
            progress.detail.set_text(
                "extremely_long_filename_with_many_segments_and_metadata_2026_11_17.tar.zst · 128.4 MB/s\nextra line",
            );
            assert_eq!(measure(), baseline);

            progress.title.set_text("");
            progress.detail.set_text("");
            assert_eq!(measure(), baseline);
        },
    );
}
