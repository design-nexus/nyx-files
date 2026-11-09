// SPDX-License-Identifier: MIT

use super::*;
use sourceview5::prelude::*;

#[test]
fn suite_syntax_palettes_load_in_gtksourceview() {
    gtk_test(
        "ui::theme::tests::syntax::suite_syntax_palettes_load_in_gtksourceview",
        || {
            let themes = nyx_theme::all();
            assert!(!themes.is_empty());
            for theme in themes {
                let tokens = super::super::tokens_from(&theme);
                let palette = super::super::resolved_source_palette(&tokens, None);
                let directory = tempfile::tempdir().expect("scheme directory");
                std::fs::write(
                    directory.path().join(format!("{}.xml", super::super::SOURCE_SCHEME)),
                    source_style_scheme_xml(&tokens, None),
                )
                .expect("write scheme");
                let manager = sourceview5::StyleSchemeManager::new();
                manager.set_search_path(&[directory.path().to_str().expect("UTF-8 scheme path")]);
                let scheme = manager.scheme(super::super::SOURCE_SCHEME).expect(&theme.id);
                // A theme without its own syntax colours gets them from its accent.
                for (style, color) in [
                    ("def:statement", palette.statement),
                    ("def:string", palette.string),
                    ("def:constant", palette.constant),
                    ("def:type", palette.type_color),
                    ("def:preprocessor", palette.preprocessor),
                ] {
                    assert_eq!(
                        scheme
                            .style(style)
                            .expect("syntax style")
                            .foreground()
                            .as_deref(),
                        Some(color_to_hex(&color).as_str()),
                        "{} {style}",
                        theme.id
                    );
                }
            }
        },
    );
}


#[test]
fn legacy_custom_themes_keep_independent_fallbacks_and_preserve_overrides() {
    let source = r##"
name = "Legacy"
background = "#000000"
surface = "#111111"
text = "#ffffff"
accent = "#336699"
muted = "#222222"
highlight = "#333333"
border = "#444444"
dim_text = "#888888"
"##;
    let mut tokens: ThemeTokens = toml::from_str(source).expect("legacy theme");
    let derived = super::super::resolved_source_palette(&tokens, None);
    tokens.syntax_string = Some("rgb(0,255,0)".into());
    let resolved = super::super::resolved_source_palette(&tokens, None);
    assert_eq!(resolved.statement, derived.statement);
    assert_eq!(resolved.type_color, derived.type_color);
    assert_eq!(resolved.constant, derived.constant);
    assert_eq!(resolved.preprocessor, derived.preprocessor);
    assert_eq!(color_to_hex(&resolved.string), "#00ff00");
    tokens.initialize_syntax_colors();
    let restored: ThemeTokens =
        toml::from_str(&toml::to_string(&tokens).expect("serialize theme")).expect("restore theme");
    assert_eq!(restored, tokens);
    assert!(validate_tokens(&restored).is_ok());
    for slot in ["keyword", "string", "constant", "type", "preprocessor"] {
        let invalid: ThemeTokens =
            toml::from_str(&format!("{source}\nsyntax_{slot} = \"not-a-color\"\n"))
                .expect("parse invalid-color fixture");
        assert!(validate_tokens(&invalid).is_err(), "{slot}");
    }
}
