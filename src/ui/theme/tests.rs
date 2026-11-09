// SPDX-License-Identifier: MIT

mod syntax;

use super::{
    SOURCE_SCHEME, ThemeManager, ThemeTokens, blend, canonical_color, color_to_hex,
    contrast::contrast, source_style_scheme_xml, suite_tokens, validate_tokens,
};
use crate::test_support::gtk_test;
use gtk::prelude::*;

#[test]
fn suite_themes_keep_dim_text_readable() {
    for theme in nyx_theme::all() {
        let tokens = super::tokens_from(&theme);
        assert!(validate_tokens(&tokens).is_ok(), "{}", theme.id);
        assert!(contrast(&tokens.dim_text, &tokens.background) >= 4.5, "{}", theme.id);
        assert!(contrast(&tokens.dim_text, &tokens.surface) >= 3.0, "{}", theme.id);
    }
}

fn wait_for_live_theme(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "live theme refresh timed out"
        );
        gtk::glib::MainContext::default().iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn has_color(label: &gtk::Label, expected: &gtk::gdk::RGBA) -> bool {
    let actual = label.color();
    (actual.red() - expected.red()).abs() < 0.01
        && (actual.green() - expected.green()).abs() < 0.01
        && (actual.blue() - expected.blue()).abs() < 0.01
}

#[test]
fn colors_can_be_blended_into_semantic_tokens() {
    assert_eq!(blend("#000000", "#ffffff", 0.5), "#808080");
    assert_eq!(blend("rgb(0,0,0)", "rgb(255,255,255)", 0.5), "#808080");
    assert_eq!(blend("#000", "#fff", 0.5), "#808080");
    assert_eq!(blend("black", "white", 0.5), "#808080");
}

#[test]
fn gtk_color_formats_canonicalize_for_persistence_and_scheme_xml() {
    for (value, css, opaque) in [
        ("rgb(153,193,241)", Some("#99c1f1"), "#99c1f1"),
        ("#fff", Some("#ffffff"), "#ffffff"),
        ("#81A1C1", Some("#81a1c1"), "#81a1c1"),
        ("rebeccapurple", Some("#663399"), "#663399"),
        ("#000aaafff", Some("#00aaff"), "#00aaff"),
        ("#0000aaaaffff", Some("#00aaff"), "#00aaff"),
        ("#0000aaaaffff8888", Some("#00aaff88"), "#00aaff"),
        ("rgba(0, 170, 255, 0.5)", Some("#00aaff80"), "#00aaff"),
        ("0x7aa2f7", None, "0x7aa2f7"),
    ] {
        assert_eq!(canonical_color(value).as_deref(), css, "{value}");
        assert_eq!(color_to_hex(value), opaque, "{value}");
    }
}

fn scheme_color_values(xml: &str) -> Vec<&str> {
    xml.lines()
        .filter_map(|line| {
            let start = line.find("value=\"")? + 7;
            let rest = line.get(start..)?;
            let end = rest.find('"')?;
            Some(&rest[..end])
        })
        .collect()
}

#[test]
fn source_style_scheme_xml_canonicalizes_rgb_tokens_for_gtksourceview() {
    gtk_test(
        "ui::theme::tests::source_style_scheme_xml_canonicalizes_rgb_tokens_for_gtksourceview",
        || {
            let tokens = ThemeTokens {
                name: "Picker".to_owned(),
                background: "rgb(255,255,255)".to_owned(),
                surface: "rgb(245,245,245)".to_owned(),
                text: "rgb(30,29,31)".to_owned(),
                accent: "rgb(153,193,241)".to_owned(),
                danger: "rgb(229,72,77)".to_owned(),
                muted: "rgb(200,200,200)".to_owned(),
                highlight: "rgb(36,77,104)".to_owned(),
                border: "rgb(49,91,117)".to_owned(),
                dim_text: "rgb(111,141,163)".to_owned(),
                syntax_keyword: None,
                syntax_string: None,
                syntax_constant: None,
                syntax_type: None,
                syntax_preprocessor: None,
            };
            let xml = source_style_scheme_xml(&tokens, None);
            let values = scheme_color_values(&xml);
            assert_eq!(values.len(), 12);
            for value in &values {
                assert!(
                    value.starts_with('#') && value.len() == 7,
                    "scheme colors must be canonical #rrggbb, got {value}"
                );
            }
            assert!(
                !xml.contains("rgb("),
                "scheme XML must not emit rgb() colour tags"
            );

            let directory = tempfile::tempdir().expect("scheme directory");
            std::fs::write(directory.path().join(format!("{SOURCE_SCHEME}.xml")), xml.as_bytes())
                .expect("write scheme");
            let manager = sourceview5::StyleSchemeManager::new();
            manager.set_search_path(&[directory.path().to_str().expect("utf-8 scheme path")]);
            manager.force_rescan();
            let scheme = manager
                .scheme(SOURCE_SCHEME)
                .expect("GtkSourceView should load a #rrggbb scheme");
            let statement = scheme
                .style("def:statement")
                .expect("def:statement should resolve");
            assert_eq!(statement.foreground().as_deref(), Some("#99c1f1"));
        },
    );
}

#[test]
fn user_stylesheet_has_the_final_word_and_reloads_live() {
    gtk_test(
        "ui::theme::tests::user_stylesheet_has_the_final_word_and_reloads_live",
        || {
            let config = crate::ui::preferences::config_directory();
            std::fs::create_dir_all(&config).expect("config directory");
            let stylesheet = config.join("style.css");

            let _manager = ThemeManager::shared();
            std::fs::write(
                &stylesheet,
                "label { color: nope; }\nlabel { color: rgb(255, 0, 0); }\n",
            )
            .expect("user stylesheet");
            let window = gtk::Window::new();
            let label = gtk::Label::new(Some("styled"));
            window.set_child(Some(&label));
            window.present();
            let red = gtk::gdk::RGBA::parse("rgb(255, 0, 0)").expect("red");
            let green = gtk::gdk::RGBA::parse("rgb(0, 255, 0)").expect("green");
            wait_for_live_theme(|| has_color(&label, &red));

            std::fs::write(&stylesheet, "label { color: rgb(0, 255, 0); }\n")
                .expect("edited stylesheet");
            wait_for_live_theme(|| has_color(&label, &green));

            std::fs::remove_file(&stylesheet).expect("remove stylesheet");
            wait_for_live_theme(|| !has_color(&label, &green));
            window.close();
        },
    );
}

#[test]
fn suite_tokens_come_from_the_files_theme() {
    let tokens = suite_tokens();
    assert!(validate_tokens(&tokens).is_ok());
    assert_eq!(tokens.name, nyx_theme::suite_theme("files").name);
}
