// SPDX-License-Identifier: MIT

use super::*;

fn ready(provider: &Provider, index: usize) {
    super::video::ready(provider, index, "audio/x-wav");
}


#[test]
fn probed_video_replaces_the_audio_view_without_stopping_playback() {
    crate::test_support::gtk_test(
        "ui::preview::tests::audio::probed_video_replaces_the_audio_view_without_stopping_playback",
        || {
            let provider = Rc::new(Provider::default());
            let drawer = PreviewDrawer::new(provider.clone(), false);
            let preferences = crate::ui::preferences::PreferenceManager::shared();
            preferences.set_preview_autoplay(true);
            drawer.show(entry("movie.ogg"), None);
            ready(&provider, 0);
            let media = drawer.state.media.borrow().clone().expect("rendered media");
            let decoded = media
                .downcast_ref::<crate::ui::media::DecodedMedia>()
                .expect("decoded stream");
            assert!(media.is_playing());
            assert_eq!(decoded.fade(), 0.0, "autoplay starts silent");
            media.stream_prepared(true, true, true, 10_000_000);
            assert!(drawer.state.audio.borrow().is_none());
            assert_eq!(drawer.state.media.borrow().as_ref(), Some(&media));
            assert!(media.is_playing());
            assert_eq!(decoded.fade(), 1.0, "the video view takes over with sound");
            preferences.set_preview_autoplay(false);
            drawer.close();
        },
    );
}

#[test]
fn autoplayed_audio_fades_in_quickly_unless_the_listener_acts() {
    crate::test_support::gtk_test(
        "ui::preview::tests::audio::autoplayed_audio_fades_in_quickly_unless_the_listener_acts",
        || {
            let provider = Rc::new(Provider::default());
            let drawer = PreviewDrawer::new(provider.clone(), false);
            let preferences = crate::ui::preferences::PreferenceManager::shared();
            preferences.set_preview_autoplay(true);
            preferences.set_preview_audio(0.8, false);
            let state = &drawer.state;
            let decoded = || {
                state
                    .media
                    .borrow()
                    .clone()
                    .and_downcast::<crate::ui::media::DecodedMedia>()
                    .expect("decoded stream")
            };
            let view = || {
                state
                    .audio
                    .borrow()
                    .as_ref()
                    .expect("audio view")
                    .view
                    .clone()
            };
            let pump = |until: &dyn Fn() -> bool, what: &str| {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while !until() {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "{what} did not happen"
                    );
                    glib::MainContext::default().iteration(false);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            };

            drawer.show(entry("a.wav"), None);
            ready(&provider, 0);
            let media = decoded();
            media.use_test_audio_stream();
            assert!(media.is_playing(), "autoplay starts the track");
            assert_eq!(media.fade(), 0.0, "silently");
            assert!(view().is_easing_in());
            pump(&|| media.fade() > 0.0, "the rise once sound flows");
            pump(&|| media.fade() == 1.0, "the half-second fade");
            assert!(!view().is_easing_in());
            assert_eq!(media.volume(), 0.8, "the saved volume is untouched");

            drawer.show(entry("b.wav"), None);
            ready(&provider, 1);
            let media = decoded();
            media.use_test_audio_stream();
            assert_eq!(media.fade(), 0.0);
            assert!(state.media_command(MediaCommand::PlayPause));
            assert_eq!(media.fade(), 1.0, "pausing brings the sound in at once");
            assert!(!view().is_easing_in());

            drawer.show(entry("c.wav"), None);
            ready(&provider, 2);
            let media = decoded();
            assert_eq!(media.fade(), 0.0, "a short track still arms silent");
            media.stream_prepared(true, false, true, 3_000_000);
            assert_eq!(
                media.fade(),
                1.0,
                "a file under ten seconds plays at full volume from the first sample"
            );
            assert!(!view().is_easing_in());

            preferences.set_preview_autoplay(false);
            drawer.close();
        },
    );
}
