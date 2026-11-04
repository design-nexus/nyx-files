// SPDX-License-Identifier: MIT

use super::*;
use crate::model::{Location, MetadataValue};

mod audio;
mod presentation;
mod find;
mod media_controls;
mod model;
mod operations;
mod pdf;
mod video;

struct Pending {
    request: PreviewRequest,
    emit: Rc<dyn Fn(PreviewEvent)>,
}

#[derive(Default)]
struct Provider(RefCell<Vec<Pending>>);

impl PreviewProvider for Provider {
    fn load(&self, request: PreviewRequest, emit: Rc<dyn Fn(PreviewEvent)>) -> LoadHandle {
        self.0.borrow_mut().push(Pending { request, emit });
        LoadHandle::new(|| {})
    }
}

pub(super) fn entry(name: &str) -> FileEntry {
    FileEntry {
        location: Location::local(name),
        thumbnail_path: None,
        native_name: name.into(),
        display_name: name.into(),
        kind: EntryKind::File,
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        mode: MetadataValue::Unknown,
        recent_unix_seconds: MetadataValue::Unknown,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        is_hidden: false,
        recent_uri: None,
    }
}

#[test]
fn comic_and_epub_selections_are_quick_preview_targets() {
    for name in ["sample.cbz", "sample.cbr", "sample.epub"] {
        assert!(preview_target(Some(entry(name))).is_some(), "{name}");
    }
}

#[test]
fn ambiguous_names_are_quick_preview_targets_for_the_loader_to_resolve() {
    for name in ["app.ts", "module.mts", "notes.zig"] {
        assert!(preview_target(Some(entry(name))).is_some(), "{name}");
    }
}

#[test]
fn model_progress_and_theme_reloads_follow_the_current_request_in_each_drawer() {
    crate::test_support::gtk_test(
        "ui::preview::tests::model_progress_and_theme_reloads_follow_the_current_request_in_each_drawer",
        || {
            let provider = Rc::new(Provider::default());
            let first = PreviewDrawer::new(provider.clone(), false);
            let second = PreviewDrawer::new(provider.clone(), false);
            let manager = super::super::theme::ThemeManager::shared();
            first.show(entry("old.stl"), None);
            second.show(entry("second.stl"), None);
            first.show(entry("new.stl"), None);
            let emit_progress = |index: usize, stage| {
                let pending = provider.0.borrow();
                let pending = &pending[index];
                (pending.emit)(PreviewEvent::Progress {
                    request_id: pending.request.id,
                    stage,
                });
            };
            emit_progress(
                2,
                crate::services::ModelPreviewStage::Rendering { triangles: 23 },
            );
            let triangles = first
                .state
                .model
                .borrow()
                .as_ref()
                .expect("loading model UI")
                .triangles
                .clone();
            assert_eq!(triangles.text(), "23 triangles");
            emit_progress(
                0,
                crate::services::ModelPreviewStage::Rendering { triangles: 99 },
            );
            assert_eq!(triangles.text(), "23 triangles");
            assert!(first.state.model_spinner.is_visible());
            {
                let pending = provider.0.borrow();
                (pending[0].emit)(PreviewEvent::Failed {
                    request_id: pending[0].request.id,
                    entry: entry("old.stl"),
                    message: "stale failure".into(),
                });
            }
            assert!(first.state.model.borrow().is_some(), "current model");
            assert!(first.state.model_spinner.is_visible());
            model::ready(&provider, 2, Some(crate::services::ModelView::FrontLeft));
            model::ready(&provider, 1, Some(crate::services::ModelView::FrontLeft));
            let next = first
                .state
                .model
                .borrow()
                .as_ref()
                .expect("mesh controls")
                .next
                .clone();
            next.emit_clicked();
            model::ready(&provider, 3, Some(crate::services::ModelView::BackLeft));
            let old_palette = manager.active_model_palette();
            let mut tokens = manager.appearance_tokens();
            tokens.accent = if old_palette.accent == 0xff0000 {
                "#00ff00"
            } else {
                "#ff0000"
            }
            .into();
            manager.preview(&tokens);
            {
                let pending = provider.0.borrow();
                assert_eq!(pending.len(), 6);
                assert_eq!(pending[4].request.entry.native_name, "new.stl");
                assert_eq!(
                    pending[4].request.model_view,
                    crate::services::ModelView::BackLeft
                );
                assert_eq!(pending[5].request.entry.native_name, "second.stl");
                assert_eq!(
                    pending[5].request.model_view,
                    crate::services::ModelView::FrontLeft
                );
                for request in &pending[4..] {
                    assert_eq!(
                        request.request.model_palette,
                        manager.active_model_palette()
                    );
                    assert_ne!(request.request.model_palette, old_palette);
                }
            }
            first.close();
            emit_progress(4, crate::services::ModelPreviewStage::Finishing);
            assert!(first.state.current_request.get().is_none());
            assert!(first.state.model.borrow().is_none());
            tokens.surface = "#102030".into();
            manager.preview(&tokens);
            assert_eq!(
                provider.0.borrow().len(),
                7,
                "closed drawer must not reload"
            );
            second.close();
        },
    );
}

struct EmptySource;

impl crate::services::FileSource for EmptySource {
    fn validate_location(
        &self,
        _location: &Location,
    ) -> Result<(), crate::services::LocationValidationError> {
        Ok(())
    }

    fn enumerate(
        &self,
        request: crate::services::DirectoryRequest,
        emit: Rc<dyn Fn(crate::services::DirectoryEvent)>,
    ) -> LoadHandle {
        emit(crate::services::DirectoryEvent::Finished {
            request_id: request.id,
            truncated: false,
            can_trash: None,
            can_delete: None,
        });
        LoadHandle::new(|| {})
    }
}




