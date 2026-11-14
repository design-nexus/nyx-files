// SPDX-License-Identifier: MIT

pub(crate) mod download;
mod image_conversion;



use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use ashpd::{
    WindowIdentifierType,
    desktop::file_chooser::FileFilter,
};
use gtk::{gio, glib, prelude::*};

use crate::{
    adapters::{LocalFileSource, probes},
    model::{FileEntry, Location},
    services::{
        DirectoryChange, DirectoryEvent, DirectoryRequest, FileSource, ItemIdentity, LoadHandle,
        LocationValidationError, MetadataRequest, StampAnswer, Watching,
    },
};



thread_local! {
    static CHOOSERS: RefCell<HashMap<String, glib::WeakRef<gtk::Window>>> = RefCell::new(HashMap::new());
    static DESTINATION_PARENTS: RefCell<Vec<glib::WeakRef<gtk::Window>>> = const { RefCell::new(Vec::new()) };
}



pub(crate) struct ChooserFileSource {
    source: Rc<dyn FileSource>,
    filter: Rc<RefCell<Option<gtk::FileFilter>>>,
    pub(crate) directory_only: Rc<Cell<bool>>,
    root_limit: Option<PathBuf>,
}

impl ChooserFileSource {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            source: Rc::new(LocalFileSource),
            filter: Rc::new(RefCell::new(None)),
            directory_only: Rc::new(Cell::new(false)),
            root_limit: None,
        })
    }

    pub(crate) fn set_filter(&self, filter: Option<gtk::FileFilter>) {
        self.filter.replace(filter);
    }

    fn refusal(&self, location: &Location) -> Result<(), LocationValidationError> {
        if !self.allows_navigation(location) {
            return Err(LocationValidationError::Refused(OUTSIDE_ROOT));
        }
        if location.native_path().is_none() && !location.is_recent_root() && !location.is_favorites_root() {
            return Err(LocationValidationError::Refused(
                "The system file chooser supports local files and folders only.",
            ));
        }
        Ok(())
    }
}

impl FileSource for ChooserFileSource {
    fn allows_navigation(&self, location: &Location) -> bool {
        chooser_location_allowed(self.root_limit.as_deref(), location)
    }

    fn allows_entry(&self, entry: &FileEntry) -> bool {
        chooser_entry_allowed(
            self.filter.borrow().as_ref(),
            self.directory_only.get(),
            self.root_limit.as_deref(),
            entry,
        )
    }

    fn validate_location(&self, location: &Location) -> Result<(), LocationValidationError> {
        self.refusal(location)?;
        if let (Some(root), Some(path)) = (&self.root_limit, location.native_path())
            && !path.canonicalize().is_ok_and(|real| real.starts_with(root))
        {
            return Err(LocationValidationError::Refused(OUTSIDE_ROOT));
        }
        self.source.validate_location(location)
    }

    fn validate_location_async(
        &self,
        location: Location,
        emit: Rc<dyn Fn(Result<(), LocationValidationError>)>,
    ) -> LoadHandle {
        if let (Some(root), Some(path)) = (self.root_limit.clone(), location.native_path())
            && self.allows_navigation(&location)
        {
            return after_containment(root, path.to_owned(), move |answer| {
                emit(answer);
                None
            });
        }
        if let Err(refusal) = self.refusal(&location) {
            emit(Err(refusal));
            return LoadHandle::new(|| {});
        }
        self.source.validate_location_async(location, emit)
    }

    fn supports_metadata_fill(&self, location: &Location) -> bool {
        (location.native_path().is_some() || location.is_recent_root() || location.is_favorites_root())
            && self.source.supports_metadata_fill(location)
    }

    fn fill_metadata(
        &self,
        request: MetadataRequest,
        emit: Rc<dyn Fn(DirectoryEvent)>,
    ) -> LoadHandle {
        self.source.fill_metadata(request, emit)
    }

    fn enumerate(&self, request: DirectoryRequest, emit: Rc<dyn Fn(DirectoryEvent)>) -> LoadHandle {
        let filter = self.filter.clone();
        let directory_only = self.directory_only.clone();
        let root_limit = self.root_limit.clone();
        let emit: Rc<dyn Fn(DirectoryEvent)> = Rc::new(move |event| {
            let event = match event {
                DirectoryEvent::Batch {
                    request_id,
                    mut entries,
                } => {
                    entries.retain(|entry| {
                        chooser_entry_allowed(
                            filter.borrow().as_ref(),
                            directory_only.get(),
                            root_limit.as_deref(),
                            entry,
                        )
                    });
                    DirectoryEvent::Batch {
                        request_id,
                        entries,
                    }
                }
                event => event,
            };
            emit(event);
        });
        let (Some(root), Some(path)) = (self.root_limit.clone(), request.location.native_path())
        else {
            return self.source.enumerate(request, emit);
        };
        let path = path.to_owned();
        let source = self.source.clone();
        after_containment(root, path, move |answer| match answer {
            Ok(()) => Some(source.enumerate(request, emit)),
            Err(error) => {
                emit(DirectoryEvent::Failed {
                    request_id: request.id,
                    message: error.message(),
                    refused: false,
                });
                None
            }
        })
    }

    fn start_watch(
        &self,
        location: Location,
        include_hidden: bool,
        notify: Rc<dyn Fn(DirectoryChange)>,
        report: Rc<dyn Fn(Watching)>,
    ) -> LoadHandle {
        let filter = self.filter.clone();
        let directory_only = self.directory_only.clone();
        let root_limit = self.root_limit.clone();
        let notify: Rc<dyn Fn(DirectoryChange)> = Rc::new(move |change| {
            notify(filter_directory_change(
                filter.borrow().as_ref(),
                directory_only.get(),
                root_limit.as_deref(),
                change,
            ));
        });
        let (Some(root), Some(path)) = (self.root_limit.clone(), location.native_path()) else {
            return self
                .source
                .start_watch(location, include_hidden, notify, report);
        };
        let path = path.to_owned();
        let source = self.source.clone();
        after_containment(root, path, move |answer| match answer {
            Ok(()) => Some(source.start_watch(location, include_hidden, notify, report)),
            Err(_) => {
                report(Watching::Unavailable);
                None
            }
        })
    }

    fn probe_stamp(&self, location: Location, emit: Rc<dyn Fn(StampAnswer)>) -> LoadHandle {
        self.source.probe_stamp(location, emit)
    }

    fn check_stamps(
        &self,
        locations: Vec<Location>,
        emit: Rc<dyn Fn(Location, StampAnswer)>,
    ) -> LoadHandle {
        self.source.check_stamps(locations, emit)
    }

    fn responsiveness(&self, location: &Location) -> crate::services::Responsiveness {
        self.source.responsiveness(location)
    }

    fn follow_responsiveness(&self, notify: Rc<dyn Fn()>) -> LoadHandle {
        self.source.follow_responsiveness(notify)
    }

    fn find_by_identity(
        &self,
        parent: Location,
        identity: ItemIdentity,
        emit: Rc<dyn Fn(Option<Location>)>,
    ) -> LoadHandle {
        self.source.find_by_identity(parent, identity, emit)
    }
}

fn file_filter_matches(filter: &gtk::FileFilter, entry: &FileEntry) -> bool {
    if entry.is_directory() {
        return true;
    }
    let info = gio::FileInfo::new();
    info.set_name(Path::new(&entry.native_name));
    info.set_display_name(&entry.display_name);
    info.set_file_type(gio::FileType::Regular);
    let (content_type, _) = crate::services::guess_content_type(Path::new(&entry.native_name));
    info.set_content_type(&content_type);
    filter.match_(&info)
}

const OUTSIDE_ROOT: &str = "Choose an existing folder inside this removable device.";

fn after_containment(
    root: PathBuf,
    path: PathBuf,
    proven: impl FnOnce(Result<(), LocationValidationError>) -> Option<LoadHandle> + 'static,
) -> LoadHandle {
    let started: Rc<RefCell<Option<LoadHandle>>> = Rc::default();
    let slot = started.clone();
    let task = glib::MainContext::default().spawn_local(async move {
        let answer = match probes::real_folder(&path).await {
            Ok(real) if real.starts_with(&root) => Ok(()),
            Ok(_) => Err(LocationValidationError::Refused(OUTSIDE_ROOT)),
            Err(error) => Err(error),
        };
        let handle = proven(answer);
        slot.replace(handle);
    });
    LoadHandle::new(move || {
        task.abort();
        started.take();
    })
}

fn chooser_location_allowed(root: Option<&Path>, location: &Location) -> bool {
    root.is_none_or(|root| {
        location.native_path().is_some_and(|path| {
            path.starts_with(root) && !path.components().any(|part| part == Component::ParentDir)
        })
    })
}

fn chooser_entry_allowed(
    filter: Option<&gtk::FileFilter>,
    directory_only: bool,
    root_limit: Option<&Path>,
    entry: &FileEntry,
) -> bool {
    chooser_location_allowed(root_limit, &entry.location)
        && entry.location.native_path().is_some()
        && (!directory_only || entry.is_directory())
        && filter.is_none_or(|filter| file_filter_matches(filter, entry))
}

fn filter_directory_change(
    filter: Option<&gtk::FileFilter>,
    directory_only: bool,
    root_limit: Option<&Path>,
    change: DirectoryChange,
) -> DirectoryChange {
    match change {
        DirectoryChange::Upsert(entry)
            if !chooser_entry_allowed(filter, directory_only, root_limit, &entry) =>
        {
            DirectoryChange::Remove(entry.location)
        }
        DirectoryChange::Move { from, entry }
            if !chooser_entry_allowed(filter, directory_only, root_limit, &entry) =>
        {
            DirectoryChange::Remove(from)
        }
        change => change,
    }
}

#[derive(Clone)]
pub(crate) struct PortalFilter {
    pub(crate) portal: FileFilter,
    pub(crate) native: gtk::FileFilter,
}




// Popovers use separate surfaces, so the window does not constrain their content height.






pub(crate) fn eligible_open_entries(entries: Vec<FileEntry>, directory: bool) -> Vec<FileEntry> {
    entries
        .into_iter()
        .filter(|entry| entry.is_directory() == directory)
        .collect()
}

const MIN_CHOOSER_WIDTH: i32 = 640;
const MIN_CHOOSER_HEIGHT: i32 = 460;
const MAX_CHOOSER_WIDTH: i32 = 1000;
const MAX_CHOOSER_HEIGHT: i32 = 680;
const FALLBACK_CHOOSER_WIDTH: i32 = 920;
const FALLBACK_CHOOSER_HEIGHT: i32 = 580;

fn chooser_default_dimensions_for_monitor(monitor_width: i32, monitor_height: i32) -> (i32, i32) {
    if monitor_width <= 0 || monitor_height <= 0 {
        return (FALLBACK_CHOOSER_WIDTH, FALLBACK_CHOOSER_HEIGHT);
    }
    let target_width = (monitor_width.saturating_mul(80) / 100)
        .min(monitor_width.saturating_sub(120))
        .clamp(MIN_CHOOSER_WIDTH.min(monitor_width), MAX_CHOOSER_WIDTH);
    let target_height = (monitor_height.saturating_mul(78) / 100)
        .min(monitor_height.saturating_sub(100))
        .clamp(MIN_CHOOSER_HEIGHT.min(monitor_height), MAX_CHOOSER_HEIGHT);

    (target_width, target_height)
}

pub(crate) fn chooser_initial_dimensions(monitor: Option<(i32, i32)>) -> (i32, i32) {
    monitor.filter(|(width, height)| *width > 0 && *height > 0).map_or(
        (FALLBACK_CHOOSER_WIDTH, FALLBACK_CHOOSER_HEIGHT),
        |(width, height)| chooser_default_dimensions_for_monitor(width, height),
    )
}

pub(crate) fn detect_monitor_geometry(
    display: Option<&gtk::gdk::Display>,
    window: Option<&gtk::Window>,
) -> Option<(i32, i32)> {
    let window_display = window.map(gtk::prelude::WidgetExt::display);
    let default_display = gtk::gdk::Display::default();
    let display = display
        .or(window_display.as_ref())
        .or(default_display.as_ref())?;

    if let Some(geom) = window
        .and_then(|w| w.surface())
        .and_then(|surface| display.monitor_at_surface(&surface))
        .map(|monitor| monitor.geometry())
        .filter(|geom| geom.width() > 0 && geom.height() > 0)
    {
        return Some((geom.width(), geom.height()));
    }

    let monitors = display.monitors();
    for index in 0..monitors.n_items() {
        if let Some(monitor) = monitors
            .item(index)
            .and_then(|item| item.downcast::<gtk::gdk::Monitor>().ok())
        {
            let geom = monitor.geometry();
            if geom.width() > 0 && geom.height() > 0 {
                return Some((geom.width(), geom.height()));
            }
        }
    }

    None
}












pub(crate) fn portal_filters(
    filters: &[FileFilter],
    current: Option<&FileFilter>,
) -> (Vec<PortalFilter>, Option<usize>) {
    let (filters, selected) = normalize_portal_filters(filters, current);
    (
        filters
            .into_iter()
            .map(|portal| {
                let native = gtk::FileFilter::new();
                native.set_name(Some(portal.label()));
                for pattern in portal.pattern_filters() {
                    native.add_pattern(pattern);
                }
                for mime in portal.mimetype_filters() {
                    native.add_mime_type(mime);
                }
                PortalFilter { portal, native }
            })
            .collect(),
        selected,
    )
}

fn normalize_portal_filters(
    filters: &[FileFilter],
    current: Option<&FileFilter>,
) -> (Vec<FileFilter>, Option<usize>) {
    let mut filters = filters.to_vec();
    if let Some(current) = current
        && !filters.contains(current)
    {
        filters.push(current.clone());
    }
    let selected = current
        .and_then(|current| filters.iter().position(|filter| filter == current))
        .or_else(|| (!filters.is_empty()).then_some(0));
    (filters, selected)
}





pub(crate) fn apply_external_parent(window: &gtk::Window, parent: Option<&WindowIdentifierType>) {
    let Some(WindowIdentifierType::Wayland(handle)) = parent else {
        return;
    };
    let Some(surface) = window.surface() else {
        return;
    };
    let Ok(toplevel) = surface.downcast::<gdk4_wayland::WaylandToplevel>() else {
        tracing::debug!("portal parent type does not match the current display backend");
        return;
    };
    if !toplevel.set_transient_for_exported(handle) {
        tracing::debug!("Wayland compositor rejected the portal parent handle");
    }
}






