// SPDX-License-Identifier: MIT

use super::*;

fn metadata_fixture(names: &[&str]) -> (tempfile::TempDir, Rc<Browser>) {
    let directory = tempfile::tempdir().expect("fixture");
    for name in names {
        std::fs::write(directory.path().join(name), b"fixture").expect("media file");
    }
    let browser = Browser::new(Rc::new(crate::adapters::LocalFileSource));
    browser.navigate(Location::local(directory.path()));
    let context = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    while browser.entry_at(0, names.len() - 1).is_none() {
        assert!(Instant::now() < deadline, "directory did not load");
        context.iteration(false);
    }
    (directory, browser)
}


