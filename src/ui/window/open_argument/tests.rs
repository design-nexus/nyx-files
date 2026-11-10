// SPDX-License-Identifier: MIT


use super::*;





#[test]
fn query_kind_classifies_a_directory() {
    crate::test_support::gtk_test(
        "ui::window::open_argument::tests::query_kind_classifies_a_directory",
        || {
            let root = tempfile::tempdir().expect("fixture");
            let file = gio::File::for_path(root.path());
            let kind = glib::MainContext::new().block_on(query_kind(&file, None));
            assert!(matches!(kind, Ok(Kind::Directory)));
        },
    );
}

#[test]
fn query_kind_classifies_a_regular_file() {
    crate::test_support::gtk_test(
        "ui::window::open_argument::tests::query_kind_classifies_a_regular_file",
        || {
            let root = tempfile::tempdir().expect("fixture");
            let path = root.path().join("open me.txt");
            std::fs::write(&path, b"hello").expect("fixture file");
            let kind =
                glib::MainContext::new().block_on(query_kind(&gio::File::for_path(&path), None));
            assert!(matches!(kind, Ok(Kind::File)));
        },
    );
}

#[test]
fn query_kind_follows_a_symlink_to_a_directory() {
    crate::test_support::gtk_test(
        "ui::window::open_argument::tests::query_kind_follows_a_symlink_to_a_directory",
        || {
            let root = tempfile::tempdir().expect("fixture");
            let target = root.path().join("target");
            std::fs::create_dir(&target).expect("fixture directory");
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&target, &link).expect("fixture symlink");
            let kind =
                glib::MainContext::new().block_on(query_kind(&gio::File::for_path(&link), None));
            assert!(matches!(kind, Ok(Kind::Directory)));
        },
    );
}

#[test]
fn query_kind_treats_a_broken_symlink_as_a_file() {
    crate::test_support::gtk_test(
        "ui::window::open_argument::tests::query_kind_treats_a_broken_symlink_as_a_file",
        || {
            let root = tempfile::tempdir().expect("fixture");
            let missing = root.path().join("missing");
            let link = root.path().join("broken");
            std::os::unix::fs::symlink(&missing, &link).expect("fixture symlink");
            let kind =
                glib::MainContext::new().block_on(query_kind(&gio::File::for_path(&link), None));
            assert!(matches!(kind, Ok(Kind::File)));
        },
    );
}

#[test]
fn query_kind_fails_a_plain_missing_path() {
    crate::test_support::gtk_test(
        "ui::window::open_argument::tests::query_kind_fails_a_plain_missing_path",
        || {
            let root = tempfile::tempdir().expect("fixture");
            let missing = root.path().join("does-not-exist");
            let kind =
                glib::MainContext::new().block_on(query_kind(&gio::File::for_path(&missing), None));
            assert!(kind.is_err());
        },
    );
}












