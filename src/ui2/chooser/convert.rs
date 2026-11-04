// SPDX-License-Identifier: MIT

//! Downloaded images the selected filter refuses: renamed to a matching
//! extension, or converted to PNG once the user agrees.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, atomic::AtomicBool, mpsc::TryRecvError},
    time::Duration,
};

use gtk::{gio, glib, prelude::*};

use super::Chooser;
use crate::{
    sandbox::Cancellation,
    services::image_conversion::{self, ImageKind},
};

impl Chooser {
    fn image_target(&self, path: &Path, kind: ImageKind) -> Option<PathBuf> {
        let filter = self
            .filter_dropdown
            .as_ref()
            .and_then(|d| self.filters.get(d.selected() as usize));
        let matches = |candidate: &Path| {
            let info = gio::FileInfo::new();
            info.set_name(Path::new(candidate.file_name().unwrap_or_default()));
            info.set_display_name(&candidate.file_name().unwrap_or_default().to_string_lossy());
            info.set_file_type(gio::FileType::Regular);
            info.set_content_type(kind.mime());
            filter.is_none_or(|filter| filter.native.match_(&info))
        };
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                kind.extensions()
                    .iter()
                    .any(|v| ext.eq_ignore_ascii_case(v))
            })
            && matches(path)
        {
            return Some(path.to_owned());
        }
        kind.extensions()
            .iter()
            .flat_map(|ext| [ext.to_string(), ext.to_ascii_uppercase()])
            .map(|extension| image_conversion::with_extension(path, &extension))
            .find(|candidate| matches(candidate))
    }

    fn after_download_step<T: Send + 'static>(
        self: &Rc<Self>,
        work: impl FnOnce() -> T + Send + 'static,
        then: impl FnOnce(&Rc<Self>, T) + 'static,
    ) {
        self.cancel_download();
        let step = Arc::new(AtomicBool::new(false));
        self.download_cancel.replace(Some(step.clone()));
        self.accept_button.set_sensitive(false);
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let answer = gio::spawn_blocking(work)
                .await
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
            let Some(c) = weak.upgrade() else { return };
            let current = c
                .download_cancel
                .borrow()
                .as_ref()
                .is_some_and(|flag| Arc::ptr_eq(flag, &step));
            if current {
                c.download_cancel.take();
                c.accept_button.set_sensitive(true);
                then(&c, answer);
            }
        });
    }

    pub(super) fn complete_download(self: &Rc<Self>, path: PathBuf) {
        let probe = path.clone();
        self.after_download_step(
            move || image_conversion::detect(&probe),
            move |c, kind| c.classify_download(path, kind),
        );
    }

    pub(super) fn reuse_download(self: &Rc<Self>, url: String, path: PathBuf) {
        let probe = path.clone();
        self.after_download_step(
            move || probe.is_file().then(|| image_conversion::detect(&probe)),
            move |c, kept| match kept {
                Some(kind) => c.classify_download(path, kind),
                None => c.download(url),
            },
        );
    }

    fn classify_download(self: &Rc<Self>, path: PathBuf, kind: Option<ImageKind>) {
        let Some(kind) = kind else {
            // A PNG-looking filename alone must not pass a PNG-only filter.
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                && self.image_target(&path, ImageKind::Png).is_some()
                && self.image_target(&path, ImageKind::Jpeg).is_none()
            {
                self.show_error(&crate::i18n::tr(
                    "This download is not a supported image. Choose a JPEG, BMP, static WebP, GIF or PNG image.",
                ));
            } else {
                self.finish_remote(path);
            }
            return;
        };
        if let Some(target) = self.image_target(&path, kind) {
            if path == target {
                self.finish_remote(target);
                return;
            }
            let (from, to) = (path.clone(), target.clone());
            self.after_download_step(
                move || std::fs::rename(&from, &to),
                move |c, renamed| match renamed {
                    Ok(()) => {
                        if let Some((_, cached)) = c.downloaded_file.borrow_mut().as_mut()
                            && cached.as_path() == path
                        {
                            *cached = target.clone();
                        }
                        c.finish_remote(target);
                    }
                    Err(error) => c.show_error(&rust_i18n::t!(
                        "Could not name the downloaded image: %{error}",
                        error = error
                    )),
                },
            );
            return;
        }
        let Some(target) = self.image_target(&path, ImageKind::Png) else {
            self.finish_remote(path);
            return;
        };
        let input = path.clone();
        self.image_job(
            "Checking image…",
            move |cancelled| image_conversion::inspect(&input, &cancelled),
            move |c, kind| c.confirm_image_conversion(path.clone(), target.clone(), kind),
        );
    }

    fn image_job<T: Send + 'static>(
        self: &Rc<Self>,
        activity: &str,
        work: impl FnOnce(Cancellation) -> Result<T, String> + Send + 'static,
        done: impl Fn(&Rc<Self>, T) + 'static,
    ) {
        self.cancel_download();
        self.clear_error();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.download_cancel.replace(Some(cancelled.clone()));
        let weak = Rc::downgrade(self);
        self.show_download_progress(
            "",
            Rc::new(move || {
                if let Some(c) = weak.upgrade() {
                    c.cancel_download();
                }
            }),
        );
        if let Some(progress) = self.download_progress.borrow().as_ref() {
            progress.set_name(&crate::i18n::tr("Image"));
            progress.set_activity(&crate::i18n::tr(activity));
        }
        self.accept_button.set_sensitive(false);
        let (sender, receiver) = std::sync::mpsc::channel();
        let token = Cancellation::from(cancelled.clone());
        std::thread::spawn(move || {
            let _ = sender.send(work(token));
        });
        let filter = self.selected_filter();
        let name = self.filename.as_ref().map(EditableExt::text);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(60), move || {
            let Some(c) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !c
                .download_cancel
                .borrow()
                .as_ref()
                .is_some_and(|flag| Arc::ptr_eq(flag, &cancelled))
            {
                return glib::ControlFlow::Break;
            }
            if c.selected_filter() != filter || c.filename.as_ref().map(EditableExt::text) != name {
                c.cancel_download();
                c.show_error(&crate::i18n::tr(
                    "The name or filter changed. Press Open to try again.",
                ));
                return glib::ControlFlow::Break;
            }
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(TryRecvError::Disconnected) => {
                    Err("Image processing stopped unexpectedly. Try again.".into())
                }
            };
            c.download_cancel.take();
            c.dismiss_download_progress();
            c.accept_button.set_sensitive(true);
            match result {
                Ok(value) => done(&c, value),
                Err(message) => c.show_error(&crate::i18n::tr(&message)),
            }
            glib::ControlFlow::Break
        });
    }

    fn confirm_image_conversion(self: &Rc<Self>, path: PathBuf, target: PathBuf, kind: ImageKind) {
        if self.completion.borrow().is_none() {
            return;
        }
        if self.dialog.borrow().is_some() {
            self.show_error(&crate::i18n::tr(
                "Close the current dialog, then press Open to convert the image.",
            ));
            return;
        }
        let filter = self.selected_filter();
        let name = self.filename.as_ref().map(EditableExt::text);
        self.confirm(
            &crate::i18n::tr("Convert image to PNG?"),
            &rust_i18n::t!(
                "This image is %{value1}, which the selected filter does not accept. Convert it to PNG?",
                value1 = kind.label()
            ),
            &crate::i18n::tr("Convert to PNG"),
            false,
            move |c| {
                if c.completion.borrow().is_none() {
                    return;
                }
                if c.selected_filter() != filter
                    || c.filename.as_ref().map(EditableExt::text) != name
                {
                    c.show_error(&crate::i18n::tr(
                        "The name or filter changed. Press Open to try again.",
                    ));
                    return;
                }
                let (path, target) = (path.clone(), target.clone());
                c.image_job(
                    "Converting to PNG…",
                    move |cancelled| image_conversion::convert(&path, &target, &cancelled),
                    |c, path| c.finish_remote(path),
                );
            },
        );
    }
}
