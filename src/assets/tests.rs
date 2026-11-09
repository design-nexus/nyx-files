// SPDX-License-Identifier: MIT

use gtk::prelude::*;

use super::icons;

#[test]
fn customization_choices_are_unique_and_whitelisted() {
    let mut names: Vec<_> = icons::CUSTOMIZATION_CHOICES
        .iter()
        .map(|(name, _)| *name)
        .collect();
    names.sort_unstable();
    names.dedup();

    assert_eq!(names.len(), icons::CUSTOMIZATION_CHOICES.len());
    assert!(
        icons::CUSTOMIZATION_CHOICES
            .iter()
            .all(|(name, label)| icons::is_customization_choice(name) && !label.is_empty())
    );
    assert!(!icons::is_customization_choice("folder-from-system-theme"));
}

#[test]
fn custom_emoji_preferences_are_bounded_and_safe_to_render() {
    assert_eq!(icons::custom_emoji("emoji:🚀"), Some("🚀"));
    assert_eq!(icons::custom_emoji("emoji:👨‍👩‍👧‍👦"), Some("👨‍👩‍👧‍👦"));
    assert_eq!(icons::custom_emoji("emoji:"), None);
    assert_eq!(icons::custom_emoji("emoji:\n"), None);
    assert_eq!(
        icons::custom_emoji(&format!("emoji:{}", "x".repeat(65))),
        None
    );
}

#[test]
fn choices_saved_under_bundled_names_keep_working() {
    assert_eq!(icons::current_choice("strata-file-text"), icons::DOCUMENTS);
    assert_eq!(icons::current_choice("strata-house"), icons::HOME);
    assert!(icons::is_customization_choice("strata-list-checks"));
    assert_eq!(icons::current_choice("emoji:🚀"), "emoji:🚀");
    assert!(!icons::is_customization_choice("strata-unknown"));
}

#[test]
fn icon_texture_cache_bounds_entries_and_preserves_lru() {
    crate::test_support::gtk_test(
        "assets::tests::icon_texture_cache_bounds_entries_and_preserves_lru",
        || {
            let mut cache = super::IconTextureCache::default();
            let format = if cfg!(target_endian = "little") {
                gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied
            } else {
                gtk::gdk::MemoryFormat::A8r8g8b8Premultiplied
            };
            let dummy_texture = gtk::gdk::MemoryTexture::new(
                1,
                1,
                format,
                &gtk::glib::Bytes::from_static(&[0, 0, 0, 0]),
                4,
            )
            .upcast::<gtk::gdk::Texture>();

            for index in 0..super::ICON_TEXTURE_CACHE_LIMIT {
                let key = (format!("icon-{index}"), "#000000".to_owned(), 16);
                cache.insert(key, dummy_texture.clone());
            }
            assert_eq!(cache.entries.len(), super::ICON_TEXTURE_CACHE_LIMIT);

            let hot_key = ("icon-0".to_owned(), "#000000".to_owned(), 16);
            for _ in 0..super::ICON_TEXTURE_CACHE_LIMIT * 5 {
                assert!(cache.get(&hot_key).is_some());
            }
            assert!(cache.recent.len() <= super::ICON_TEXTURE_CACHE_LIMIT * 4);

            let new_key = ("icon-new".to_owned(), "#000000".to_owned(), 16);
            cache.insert(new_key.clone(), dummy_texture.clone());

            assert!(cache.get(&hot_key).is_some());
            let evicted_key = ("icon-1".to_owned(), "#000000".to_owned(), 16);
            assert!(cache.get(&evicted_key).is_none());
            assert!(cache.get(&new_key).is_some());
            assert_eq!(cache.entries.len(), super::ICON_TEXTURE_CACHE_LIMIT);

            cache.clear();
            assert!(cache.get(&hot_key).is_none());
            assert!(cache.get(&new_key).is_none());
            cache.insert(hot_key.clone(), dummy_texture);
            assert!(cache.get(&hot_key).is_some());
        },
    );
}

#[test]
fn symbolic_theme_icons_take_the_requested_colour() {
    crate::test_support::gtk_test(
        "assets::tests::symbolic_theme_icons_take_the_requested_colour",
        || {
            let Some(texture) = super::theme_texture(icons::COPY, 16, 1, Some("#ff0000")) else {
                // A display without the icon has nothing to recolour.
                return;
            };
            let stride = texture.width() as usize * 4;
            let mut data = vec![0_u8; stride * texture.height() as usize];
            texture.download(&mut data, stride);
            let opaque: Vec<_> = data.chunks(4).filter(|pixel| pixel[3] == 255).collect();
            assert!(!opaque.is_empty(), "the icon draws something");
            // Native-endian premultiplied ARGB: blue, green, red, alpha.
            assert!(opaque.iter().all(|pixel| pixel[2] == 255 && pixel[0] == 0 && pixel[1] == 0));
        },
    );
}
