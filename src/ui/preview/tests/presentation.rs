// SPDX-License-Identifier: MIT
use super::*;
use crate::ui::preferences::PreviewStyle;

#[test]
fn preview_presentation_one_popup_shows_and_style_changes_cancel_old_loads() {
    crate::test_support::gtk_test(
        "ui::preview::tests::presentation::preview_presentation_one_popup_shows_and_style_changes_cancel_old_loads",
        || {
            let application = gtk::Application::builder()
                .application_id("io.github.design_nexus.nyx.Files.PreviewLifecycleTest")
                .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
                .build();
            application.register(None::<&gtk::gio::Cancellable>).unwrap();
            let parent = gtk::ApplicationWindow::builder().application(&application).build();
            let split = gtk::Paned::new(gtk::Orientation::Horizontal);
            parent.set_child(Some(&split));
            let provider = Rc::new(Provider::default());
            let first = PreviewDrawer::new(provider.clone(), false);
            let second = PreviewDrawer::new(provider, false);
            first.state.change_presentation(PreviewStyle::Popup);
            second.state.change_presentation(PreviewStyle::Popup);
            first.state.panel_split.replace(Some(split));
            first.show(entry("/tmp/first.txt"), None);
            let window = first.state.popup.borrow().clone().expect("popup opens");
            assert!(window.is_visible());
            assert!(window.has_css_class("files-window"));
            assert!(window.application().is_none(), "hidden previews must not keep Files running");
            assert_eq!(application.windows().len(), 1);
            second.show(entry("/tmp/second.txt"), None);
            assert!(!first.is_enabled());
            assert!(!window.is_visible(), "one popup shows at a time");
            let second_window = second.state.popup.borrow().clone().expect("second popup opens");
            assert_ne!(second_window, window);
            assert!(second_window.is_visible());
            let request = second.state.current_request.get();
            second.state.change_presentation(PreviewStyle::Panel);
            assert!(second.state.popup.borrow().is_none());
            assert!(second.is_enabled());
            assert!(!second.state.floating.get());
            assert_ne!(second.state.current_request.get(), request);
            second.close();
            assert!(!second.is_enabled());
            assert!(second.state.load.borrow().is_none());
            drop(first);
            assert!(!window.is_visible(), "a dropped preview takes its popup with it");
            parent.destroy();
            assert!(application.windows().is_empty(), "closing the browser must release all application windows");
        },
    );
}

#[test]
fn popup_steps_through_the_host_listing_and_an_empty_one_is_harmless() {
    crate::test_support::gtk_test(
        "ui::preview::tests::presentation::popup_steps_through_the_host_listing_and_an_empty_one_is_harmless",
        || {
            let preview = PreviewDrawer::new(Rc::new(Provider::default()), false);
            let listing = Rc::new(RefCell::new(Vec::<FileEntry>::new()));
            let cursor = Rc::new(RefCell::new(None::<Location>));
            let (entries, current, selected) = (listing.clone(), listing.clone(), cursor.clone());
            preview.set_host(PreviewHost {
                entries: Box::new(move || entries.borrow().clone()),
                cursor: Box::new(move || {
                    let at = selected.borrow().clone()?;
                    current.borrow().iter().find(|e| e.location == at).cloned()
                }),
                select: {
                    let cursor = cursor.clone();
                    Box::new(move |location| *cursor.borrow_mut() = Some(location.clone()))
                },
                focus_listing: Box::new(|| {}),
            });
            preview.state.step_listing(1);
            assert!(cursor.borrow().is_none(), "an empty listing has nothing to step to");

            listing.replace(vec![entry("/tmp/a.txt"), entry("/tmp/b.txt")]);
            preview.state.step_listing(1);
            assert_eq!(*cursor.borrow(), Some(Location::local("/tmp/a.txt")));
            preview.state.step_listing(1);
            assert_eq!(*cursor.borrow(), Some(Location::local("/tmp/b.txt")));
            preview.state.step_listing(1);
            assert_eq!(*cursor.borrow(), Some(Location::local("/tmp/b.txt")), "stops at the end");
            preview.state.step_listing(-5);
            assert_eq!(*cursor.borrow(), Some(Location::local("/tmp/a.txt")), "stops at the start");
        },
    );
}
