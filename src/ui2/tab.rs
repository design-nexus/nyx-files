// SPDX-License-Identifier: MIT

//! One tab: an `app::Browser` and the view showing its folder.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};

use crate::{
    adapters::{LocalFileSource, LocalOperationProvider},
    app::{Browser, BrowserEvent},
    model::{FileEntry, Location, MetadataValue},
};

use super::list::ListView;

type Changed = Box<dyn Fn()>;

pub(super) struct Tab {
    pub(super) browser: Rc<Browser>,
    pub(super) list: ListView,
    pub(super) page: gtk::Stack,
    status: gtk::Label,
    refresh_queued: Cell<bool>,
    shown: RefCell<Option<(Location, usize)>>,
    changed: RefCell<Vec<Changed>>,
}

pub(super) fn home() -> PathBuf {
    glib::home_dir()
}

impl Tab {
    pub(super) fn new(location: Location) -> Rc<Tab> {
        let browser = Browser::with_preferences(
            Rc::new(LocalFileSource),
            crate::ui::preferences::PreferenceManager::shared().sort_preferences(),
        );
        browser.set_operation_provider(Rc::new(LocalOperationProvider));
        let list = ListView::new();
        let page = gtk::Stack::new();
        page.add_named(&list.root, Some("list"));
        let status = gtk::Label::new(None);
        status.add_css_class("dim-label");
        status.set_wrap(true);
        status.set_justify(gtk::Justification::Center);
        page.add_named(&status, Some("status"));
        let tab = Rc::new(Tab {
            browser,
            list,
            page,
            status,
            refresh_queued: Cell::new(false),
            shown: RefCell::new(None),
            changed: Default::default(),
        });
        let weak = Rc::downgrade(&tab);
        tab.browser.observe(move |event| {
            if let Some(tab) = weak.upgrade() {
                tab.on_event(event);
            }
        });
        let weak = Rc::downgrade(&tab);
        tab.list.view.connect_activate(move |_, position| {
            let Some(tab) = weak.upgrade() else { return };
            if let Some(entry) = tab.list.entry(position) {
                tab.open(&entry);
            }
        });
        let weak = Rc::downgrade(&tab);
        tab.list.on_bind(move |position, entry| {
            let Some(tab) = weak.upgrade() else { return };
            if entry.modified_unix_seconds == MetadataValue::Unknown
                && let Some(depth) = tab.depth()
            {
                tab.browser.request_visible_metadata_fill(depth, position, entry.location.clone(), false);
            }
        });
        tab.browser.navigate(location);
        tab
    }

    /// The tab's folder or title changed (for the tab row and header).
    pub(super) fn connect_changed(&self, f: impl Fn() + 'static) {
        self.changed.borrow_mut().push(Box::new(f));
    }

    pub(super) fn location(&self) -> Option<Location> {
        self.depth().and_then(|depth| self.browser.location_at(depth))
    }

    pub(super) fn title(&self) -> String {
        match self.location() {
            Some(location) if location.native_path() == Some(home().as_path()) => crate::i18n::tr("Home"),
            Some(location) => location.display_name(),
            None => crate::i18n::tr("Files"),
        }
    }

    fn depth(&self) -> Option<usize> {
        self.browser.active_depth()
    }

    fn on_event(self: &Rc<Self>, event: &BrowserEvent) {
        if let BrowserEvent::OpenRequested { .. } = event {
            return;
        }
        if self.refresh_queued.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(tab) = weak.upgrade() {
                tab.refresh_queued.set(false);
                tab.refresh();
            }
        });
    }

    fn refresh(&self) {
        let Some(depth) = self.depth() else { return };
        let Some(snapshot) = self.browser.column_snapshot(depth) else { return };
        if let Some(error) = &snapshot.error {
            self.status.set_text(error);
            self.page.set_visible_child_name("status");
        } else {
            self.page.set_visible_child_name("list");
        }
        let key = (snapshot.location.clone(), snapshot.count);
        let moved = self.shown.borrow().as_ref().map(|(l, _)| l) != Some(&key.0);
        let show_hidden = self.browser.column_preferences(depth).is_some_and(|p| p.show_hidden);
        self.browser.with_column_entries(depth, |entries| self.list.set_entries(entries, show_hidden));
        *self.shown.borrow_mut() = Some(key);
        if moved {
            self.list.view.scroll_to(0, None, gtk::ListScrollFlags::NONE, None);
            for f in self.changed.borrow().iter() {
                f();
            }
        }
    }

    pub(super) fn open(&self, entry: &FileEntry) {
        if entry.is_directory() {
            self.browser.navigate(entry.location.clone());
            return;
        }
        let file = match entry.location.native_path() {
            Some(path) => gio::File::for_path(path),
            None => match entry.location.uri_value() {
                Some(uri) => gio::File::for_uri(uri),
                None => return,
            },
        };
        let uri = file.uri();
        if let Err(error) = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>) {
            tracing::warn!(%error, "couldn't open {uri}");
        }
    }

    /// The breadcrumbs for the folder showing: from Home when inside it, else from `/`.
    pub(super) fn crumbs(&self) -> (Vec<nyx_ui::pathbar::Crumb>, String) {
        let Some(location) = self.location() else { return (Vec::new(), String::new()) };
        let home = home();
        let mut crumbs = Vec::new();
        let mut current = Some(location.clone());
        while let Some(here) = current {
            let path = here.native_path().map(PathBuf::from);
            if path.as_deref() == Some(home.as_path()) {
                crumbs.push(nyx_ui::pathbar::Crumb {
                    label: crate::i18n::tr("Home"),
                    icon: Some("user-home-symbolic".into()),
                    target: home.display().to_string(),
                });
                break;
            }
            let root = path.as_deref() == Some(std::path::Path::new("/"));
            crumbs.push(nyx_ui::pathbar::Crumb {
                label: if root { String::new() } else { here.display_name() },
                icon: root.then(|| "drive-harddisk-symbolic".into()),
                target: target_of(&here),
            });
            current = here.parent();
        }
        crumbs.reverse();
        (crumbs, location.display_path())
    }
}

fn target_of(location: &Location) -> String {
    match location.native_path() {
        Some(path) => path.display().to_string(),
        None => location.uri_value().unwrap_or_default().to_string(),
    }
}

/// A crumb's target back to a location.
pub(super) fn location_of(target: &str) -> Location {
    if target.starts_with('/') {
        Location::local(target)
    } else {
        Location::uri(target)
    }
}
