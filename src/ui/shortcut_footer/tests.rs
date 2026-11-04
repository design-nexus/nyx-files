// SPDX-License-Identifier: MIT

use super::*;

fn press(key: gdk::Key, modifiers: gdk::ModifierType) -> KeyPress {
    KeyPress::from_signal(key, 0, modifiers)
}


impl ShortcutFooter {
    pub(crate) fn assert_hints_visible(&self, visible: bool) {
        assert_eq!(
            self.widget().is_visible(),
            visible || self.paste.is_visible() || self.count.is_visible()
        );
        assert_eq!(self.more.is_visible(), visible);
    }
}

#[test]
fn localized_reference_names_keys_in_the_interface_language() {
    crate::test_support::gtk_test(
        "ui::shortcut_footer::tests::localized_reference_names_keys_in_the_interface_language",
        || {
            let manager = super::super::preferences::PreferenceManager::shared();
            // The shared manager applies the saved startup language first.
            rust_i18n::set_locale("de");
            manager.set_tenxer_mode(true);
            let footer = ShortcutFooter::new(BrowserMode::Columns);
            footer.bind_preferences(&manager);
            settle();
            let labels = reference_labels(&footer);
            let preview = crate::i18n::tr("Enter its preview; keys move into the drawer");
            assert!(reference_pairs(&labels, "l / → auf einer Datei", &preview));
            assert!(labels.iter().any(|label| label == "g g"));
            assert!(
                labels
                    .iter()
                    .any(|label| label == "Tab in Gehe zu › / Verschieben nach › / Kopieren nach ›")
            );
            for (keys, action) in [
                (
                    "d / Entf",
                    "Nach Bestätigung in den Papierkorb verschieben; d d bestätigt",
                ),
                (
                    "Strg+C / Strg+X",
                    "Ausgewählte Elemente kopieren / ausschneiden",
                ),
                (
                    "Strg+Umschalt+1–9 / 0",
                    "Tab auswählen (Strg+Umschalt für Zahlen gedrückt halten)",
                ),
            ] {
                assert!(reference_pairs(&labels, keys, action), "{keys}: {labels:?}");
            }
            footer.search.set_text("auf einer Datei");
            settle();
            let matches = reference_labels(&footer);
            assert!(reference_pairs(&matches, "l / → auf einer Datei", &preview));
            assert!(!matches.iter().any(|label| label == "g g"));
            footer.search.set_text("ctrl+c");
            settle();
            let matches = reference_labels(&footer);
            assert!(
                matches.iter().any(|label| label == "Strg+C / Strg+X"),
                "English key names find their German labels: {matches:?}"
            );
            assert!(!matches.iter().any(|label| label == "d / Entf"));
            footer.search.set_text("shift");
            settle();
            let matches = reference_labels(&footer);
            assert!(
                reference_pairs(
                    &matches,
                    ", a / , m / , s / , e",
                    "Nach Name / Änderung / Größe / Typ sortieren; Umschalt kehrt die Reihenfolge um"
                ),
                "English key names find keys named in German action text: {matches:?}"
            );
            for query in ["strg+0", "ctrl+0"] {
                footer.search.set_text(query);
                settle();
                let matches = reference_labels(&footer);
                assert!(
                    reference_pairs(
                        &matches,
                        "Strg++ / − / 0",
                        "Textgröße erhöhen / verringern / zurücksetzen"
                    ),
                    "{query} finds an alternative that shares its modifiers: {matches:?}"
                );
            }
            for (query, keys) in [
                ("ctrl+-", "Strg++ / − / 0"),
                ("g -", "g + / g −"),
                ("1-9", "Strg+Umschalt+1–9 / 0"),
            ] {
                footer.search.set_text(query);
                settle();
                let matches = reference_labels(&footer);
                assert!(
                    matches.iter().any(|label| label == keys),
                    "a typed - finds {keys}: {matches:?}"
                );
            }
        },
    );
}


fn press_reference(footer: &ShortcutFooter, key: gdk::Key) {
    let controllers = footer.panel.observe_controllers();
    let controller = (0..controllers.n_items())
        .filter_map(|index| {
            controllers
                .item(index)
                .and_downcast::<gtk::EventControllerKey>()
        })
        .next()
        .expect("reference key controller");
    assert!(
        controller.emit_by_name::<bool>("key-pressed", &[&key, &0u32, &gdk::ModifierType::empty()]),
        "{key:?} should be handled by the open reference"
    );
}

fn reference_pairs(labels: &[String], key: &str, action: &str) -> bool {
    labels
        .windows(2)
        .any(|pair| pair[0] == action && pair[1] == key)
}

fn reference_labels(footer: &ShortcutFooter) -> Vec<String> {
    let mut labels = Vec::new();
    collect_label_text(footer.reference.upcast_ref(), &mut labels);
    labels
}

fn collect_label_text(widget: &gtk::Widget, labels: &mut Vec<String>) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        labels.push(label.text().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_label_text(&current, labels);
        child = current.next_sibling();
    }
}

fn settle() {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < until {
        while glib::MainContext::default().iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn paste_availability_tracks_file_clipboard() {
    crate::test_support::gtk_test(
        "ui::shortcut_footer::tests::paste_availability_tracks_file_clipboard",
        || {
            let clipboard = gdk::Display::default().expect("test display").clipboard();
            let files = gdk::FileList::from_array(&[gtk::gio::File::for_path(
                "/tmp/strata-clipboard-fixture.txt",
            )]);
            let provider = gdk::ContentProvider::for_value(&files.to_value());
            clipboard
                .set_content(Some(&provider))
                .expect("file clipboard");
            let footer = ShortcutFooter::new(BrowserMode::Columns);
            let handler = footer.connect_clipboard(&clipboard);
            let manager = super::super::preferences::PreferenceManager::shared();
            manager.set_show_keybinding_hints(false);
            footer.bind_preferences(&manager);
            footer.assert_hints_visible(false);
            settle();
            assert!(
                footer.paste.is_visible(),
                "existing files on the clipboard enable paste"
            );
            assert!(footer.widget().is_visible());
            clipboard.set_text("plain text is not a file clipboard");
            settle();
            assert!(!footer.paste.is_visible());
            assert!(!footer.widget().is_visible());
            let uri = gdk::ContentProvider::for_bytes(
                "text/uri-list",
                &glib::Bytes::from_static(b"file:///tmp/strata-clipboard-fixture.txt\r\n"),
            );
            clipboard.set_content(Some(&uri)).expect("URI clipboard");
            settle();
            assert!(
                footer.paste.is_visible(),
                "external URI lists also enable paste"
            );
            assert!(footer.widget().is_visible());
            clipboard
                .set_content(Some(&provider))
                .expect("pending file clipboard");
            clipboard.set_text("newer clipboard replaces a pending file read");
            settle();
            assert!(!footer.paste.is_visible());
            clipboard
                .set_content(Some(&provider))
                .expect("cut clipboard");
            settle();
            assert!(footer.paste.is_visible());
            clipboard
                .set_content(None::<&gdk::ContentProvider>)
                .expect("cleared clipboard");
            settle();
            assert!(
                !footer.paste.is_visible(),
                "consuming a cut clears paste availability"
            );
            assert!(!footer.widget().is_visible());
            let empty: Option<gdk::FileList> = None;
            clipboard
                .set_content(Some(&gdk::ContentProvider::for_value(&empty.to_value())))
                .expect("empty file clipboard");
            settle();
            assert!(!footer.paste.is_visible());
            clipboard.disconnect(handler);
            clipboard
                .set_content(None::<&gdk::ContentProvider>)
                .expect("fixture cleanup");
        },
    );
}
