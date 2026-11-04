// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    env,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use gtk::{
    gdk::ModifierType,
    gio,
    gio::prelude::{EmblemedIconExt as _, VfsExt as _},
    glib,
    prelude::*,
};

use crate::{
    adapters::{
        LocalFileSource, LocalOperationProvider, RevealRequest, location_for_file,
        mount_can_unplug, mount_lanes, probes, volume_can_unplug,
    },
    app::{Browser, BrowserEvent},
    model::Location,
    services::LocationValidationError,
};

use super::{
    browser::{
        BrowserView, PeekBehavior, PinStatus, WeakBrowserView,
        show_error_dialog,
    },
    browser_modes::BrowserMode,
    controls::{ModalTone, focus_button, message_dialog_description, message_dialog_layout},
    keys::{
        KeyPress,
        bindings::{
            BACKSPACE, DOWN, ENTER, FILTER_SLASH, FIRST_ITEM, FOCUS_NEXT, LAST_ITEM, LEFT,
            PAGE_DOWN, PAGE_UP, RIGHT, SPACE, UP, VIEW_COLUMNS, VIEW_ICONS, VIEW_LIST, VIM_DOWN,
            VIM_LEFT, VIM_RIGHT, VIM_UP,
        },
    },
    modal::{ModalHost, close_on_escape, dismiss_modal_layer, modal_layer},
    motion::{animations_enabled, emphasized_deceleration},
    preferences::PreferenceManager,
};

mod bookmarks;
pub(crate) mod cloud;
mod composition;
mod density_slider;
mod device_labels;
pub(crate) mod device_release;
pub(crate) mod devices;
pub(crate) mod drive_dialogs;
pub(crate) mod drive_ops;
pub(crate) mod keyboard;
mod network_location;
pub(crate) mod open_argument;
mod pinned_drop;
mod sidebar;
mod sidebar_drop_feedback;
pub(crate) mod unlock_argument;
pub(crate) mod volume_password;

#[cfg(test)]
pub(super) use devices::offer_removable_destinations_for_test;
pub(super) use devices::{
    RemovableDestination, removable_destinations,
    send_to_recent_folders, };
pub use unlock_argument::{UnlockTarget};

pub(super) const SIDEBAR_WIDTH: i32 = 201;
pub(super) const MIN_SIDEBAR_WIDTH: i32 = 169;
pub(super) fn sidebar_rail_button_size() -> i32 {
    // Match the compact header toggle; the rail supplies its own side gutters.
    (24.0 * PreferenceManager::shared().interface_scale()).round() as i32
}

pub(super) fn sidebar_rail_width() -> i32 {
    sidebar_rail_button_size() + 10
}

/// Whether the full sidebar fits beside the content, and whether a preview
/// took part in that judgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui) struct SidebarFit {
    pub(in crate::ui) fits: bool,
    pub(in crate::ui) preview: bool,
}

/// Decides when the sidebar shows as an icon rail. The user's explicit choice
/// wins; a change in available space compacts an expanded sidebar once, and
/// space releases only what it compacted.
#[derive(Default)]
pub(in crate::ui) struct RailPolicy {
    compact: Cell<bool>,
    automatic: Cell<bool>,
    fit: Cell<Option<SidebarFit>>,
}

impl RailPolicy {
    pub(in crate::ui) fn rail(&self) -> bool {
        self.compact.get() || self.automatic.get()
    }


    /// Returns the new rail state when a changed fit changes what is shown.
    pub(in crate::ui) fn observe(&self, fit: SidebarFit) -> Option<bool> {
        if self.fit.replace(Some(fit)) == Some(fit) {
            return None;
        }
        let before = self.rail();
        if fit.fits {
            self.automatic.set(false);
        } else if !self.compact.get() {
            self.automatic.set(true);
        }
        (self.rail() != before).then(|| self.rail())
    }
}

/// The fit rule while no preview takes part: the full sidebar beside one column.
/// `None` until the window has a width.
pub(in crate::ui) fn sidebar_fits_without_preview(content: &gtk::Paned) -> Option<bool> {
    let available = content
        .root()
        .map_or_else(|| content.width(), |root| root.width());
    (available > 0).then(|| available > preferred_sidebar_width() + super::browser::COLUMN_WIDTH)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecentAvailability {
    platform_tracking_enabled: bool,
    runtime_backend_supported: bool,
}

impl RecentAvailability {

}














pub(super) fn preferred_sidebar_width() -> i32 {
    (f64::from(SIDEBAR_WIDTH) * PreferenceManager::shared().interface_scale())
        .round()
        .max(f64::from(SIDEBAR_WIDTH)) as i32
}



// Below this minimum, GTK can allocate more width than the divider position allows.
fn sidebar_minimum_width(sidebar: &gtk::Widget) -> i32 {
    let (minimum, _, _, _) = sidebar.measure(gtk::Orientation::Horizontal, -1);
    minimum
}







pub(super) fn visible_modal_layer(window: &impl IsA<gtk::Window>) -> Option<gtk::Widget> {
    topmost_modal_layer(window, &|_| false)
}

fn topmost_modal_layer(
    window: &impl IsA<gtk::Window>,
    skip: &dyn Fn(&gtk::Widget) -> bool,
) -> Option<gtk::Widget> {
    fn visible_layer(
        root: &gtk::Widget,
        skip: &dyn Fn(&gtk::Widget) -> bool,
    ) -> Option<gtk::Widget> {
        if !root.is_visible()
            || !root.is_child_visible()
            || root.opacity() == 0.0
            || root.has_css_class("tab-strip")
        {
            return None;
        }
        if root.has_css_class("app-modal-layer") {
            return (!skip(root)).then(|| root.clone());
        }
        let mut child = root.last_child();
        while let Some(widget) = child {
            child = widget.prev_sibling();
            if root.has_css_class("tab-context") && !widget.has_css_class("app-modal-layer") {
                continue;
            }
            if let Some(layer) = visible_layer(&widget, skip) {
                return Some(layer);
            }
        }
        None
    }
    let root = window.child()?;
    if root.has_css_class("tab-window") {
        visible_layer(&root, skip)
    } else {
        let mut child = root.last_child();
        while let Some(widget) = child {
            child = widget.prev_sibling();
            if widget.is_visible() && widget.has_css_class("app-modal-layer") && !skip(&widget) {
                return Some(widget);
            }
        }
        None
    }
}

/// True when the topmost visible modal layer is not `own`. An action that opens a
/// modal passes its own layer so it can still toggle that layer closed. A layer that
/// is animating out no longer owns input.
pub(super) fn foreign_modal_visible(
    window: &impl IsA<gtk::Window>,
    own: Option<&gtk::Widget>,
) -> bool {
    topmost_modal_layer(window, &crate::ui::modal::is_closing_layer)
        .is_some_and(|layer| own != Some(&layer))
}










type RailListener = Rc<dyn Fn(bool)>;

pub(super) struct SidebarState {
    widget: gtk::Box,
    view: BrowserView,
    browser: Rc<Browser>,
    volume_monitor: gio::VolumeMonitor,
    mount_monitor: gio_unix::MountMonitor,
    preference_manager: Rc<super::preferences::PreferenceManager>,
    place_order: RefCell<Vec<&'static str>>,
    places_visibility: RefCell<[bool; 10]>,
    pinned_places: Rc<RefCell<Vec<(Location, String)>>>,
    place_rows: RefCell<Vec<(Location, gtk::Button)>>,
    trash_contents: Cell<TrashContents>,
    trash_menu_rows: RefCell<Option<TrashMenuRows>>,
    trash_monitor: RefCell<Option<crate::services::LoadHandle>>,
    trash_probe_running: Cell<bool>,
    trash_probe_pending: Cell<bool>,
    local_only: bool,
    recent_availability: Cell<RecentAvailability>,
    pending_scroll: Cell<Option<f64>>,
    rebuild_queued: Cell<bool>,
    scroll_restore_queued: Cell<bool>,
    pub(in crate::ui) rail: Cell<bool>,
    policy: RailPolicy,
    pub(in crate::ui) saved_width: Cell<Option<i32>>,
    rail_changed: RefCell<Option<RailListener>>,
    keycaps: RefCell<Vec<gtk::Label>>,
    keycaps_shown: Cell<bool>,
    visible_pins: RefCell<Vec<Location>>,
    /// The separator, heading and drop gap of a Pinned section with no pins.
    empty_pinned_section: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
    file_drag_over: Cell<bool>,
    drop_feedback: Rc<sidebar_drop_feedback::SidebarDropFeedback>,
}

/// Rows of the Trash sidebar context menu that only make sense while Trash holds items.
struct TrashMenuRows {
    separator: gtk::Separator,
    empty: gtk::Button,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrashContents {
    /// Not probed yet, or the probe failed.
    Unknown,
    Empty,
    NonEmpty,
}









impl SidebarState {


    pub(in crate::ui) fn set_rail(&self, rail: bool) {
        let changed = self.rail.replace(rail) != rail;
        if !changed && !rail {
            return;
        }
        if changed && let Some(listener) = self.rail_changed.borrow().clone() {
            listener(rail);
        }
        if let Some(scroller) = self.sidebar_scroller()
            && let Some(shell) = scroller.parent()
        {
            scroller.set_overlay_scrolling(rail);
            if rail {
                shell.add_css_class("sidebar-rail");
            } else {
                shell.remove_css_class("sidebar-rail");
            }
            if let Some(paned) = shell.parent().and_downcast::<gtk::Paned>() {
                paned.set_wide_handle(!rail);
                if let Some(handle) = paned_separator(&paned) {
                    handle.set_cursor_from_name(if rail {
                        Some("default")
                    } else {
                        Some("col-resize")
                    });
                }
            }
            scroller.set_width_request(if rail {
                sidebar_rail_width()
            } else {
                MIN_SIDEBAR_WIDTH
            });
            shell.set_size_request(
                if rail {
                    sidebar_rail_width()
                } else {
                    MIN_SIDEBAR_WIDTH
                },
                -1,
            );
        }
        self.sync_rail_rows();
    }

    fn sync_rail_rows(&self) {
        let rail = self.rail.get();
        let mut widget_child = self.widget.first_child();
        while let Some(child) = widget_child {
            widget_child = child.next_sibling();
            if let Ok(heading) = child.clone().downcast::<gtk::Label>() {
                heading.set_visible(!rail);
            } else if let Ok(button) = child.clone().downcast::<gtk::Button>() {
                sync_sidebar_button(&button, rail);
            } else if child.has_css_class("sidebar-device") {
                let mut dev_child = child.first_child();
                while let Some(w) = dev_child {
                    dev_child = w.next_sibling();
                    if let Ok(button) = w.clone().downcast::<gtk::Button>() {
                        sync_sidebar_button(&button, rail);
                    } else if w.has_css_class("sidebar-device-actions") {
                        w.set_visible(!rail);
                    }
                }
            }
        }
        self.sync_keycaps();
    }



    /// Feeds the fit rule; the caller applies a returned change.
    pub(in crate::ui) fn observe_fit(&self, fit: SidebarFit) -> Option<bool> {
        self.policy.observe(fit)
    }

    /// Shows the rail or the full sidebar and moves the divider with it.
    pub(in crate::ui) fn apply_rail(&self, rail: bool) {
        self.set_rail(rail);
        let Some(shell) = self
            .sidebar_scroller()
            .and_then(|scroller| scroller.parent())
        else {
            return;
        };
        let Some(paned) = shell.parent().and_downcast::<gtk::Paned>() else {
            return;
        };
        if !shell.is_visible() {
            return;
        }
        let target = if rail {
            sidebar_rail_width()
        } else {
            self.saved_width
                .get()
                .unwrap_or_else(preferred_sidebar_width)
                .max(sidebar_minimum_width(&shell))
        };
        if paned.position() != target {
            paned.set_position(target);
        }
    }
}

fn sync_sidebar_button(button: &gtk::Button, rail: bool) {
    let size = if rail { sidebar_rail_button_size() } else { -1 };
    button.set_size_request(size, size);
    if let Some(content) = button.child() {
        let mut child = content.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                if label.has_css_class("sidebar-keycap") {
                    continue;
                }
                button.set_tooltip_text(rail.then_some(label.label().as_str()));
                if rail {
                    crate::ui::accessibility::set_stand_in_label(button, &label.label());
                } else {
                    crate::ui::accessibility::clear_stand_in_label(button);
                }
                label.set_visible(!rail);
            }
        }
        content.set_halign(if rail {
            gtk::Align::Center
        } else {
            gtk::Align::Fill
        });
    }
}

fn paned_separator(paned: &gtk::Paned) -> Option<gtk::Widget> {
    let mut child = paned.first_child();
    while let Some(widget) = child {
        if widget.css_name() == "separator" {
            return Some(widget);
        }
        child = widget.next_sibling();
    }
    None
}

impl SidebarState {
    fn sidebar_scroller(&self) -> Option<gtk::ScrolledWindow> {
        self.widget
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast()
    }


















    #[cfg(test)]
    pub(super) fn places_for_test(&self) -> gtk::Box {
        self.widget.clone()
    }
























    fn sync_keycaps(&self) {
        let visible = self.keycaps_shown.get() && !self.rail.get();
        for keycap in self.keycaps.borrow().iter() {
            sync_keycap(keycap, visible);
        }
    }








}























/// Safe-removal action for a sidebar device row. Eject is preferred whenever
/// the drive reports it (USB sticks, optical discs); plain unmount covers
/// mounts without ejectable media. `None` means fixed internal storage with
/// no actionable release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MediaRelease {
    EjectVolume,
    EjectMount,
    UnmountMount,
}







fn media_release_error_title(action: MediaRelease) -> &'static str {
    match action {
        MediaRelease::EjectVolume | MediaRelease::EjectMount => "Unable to eject device",
        MediaRelease::UnmountMount => "Unable to unmount device",
    }
}



fn navigate_home_if_within(browser: &Rc<Browser>, root: &gio::File) {
    let Some(device) = location_for_file(root) else {
        return;
    };
    let active = browser.active_location();
    if active
        .as_ref()
        .is_some_and(|active| active == &device || active.is_within(&device))
    {
        browser.navigate(Location::local(home_directory()));
    }
}






pub(crate) fn crypto_password_uuid_for_volume(volume: &gio::Volume) -> Option<String> {
    let unix = gio_volume_unix_device(volume);
    let hint = unix
        .as_deref()
        .map(devices::block_crypto_hint)
        .unwrap_or_default();
    devices::crypto_password_uuid(
        volume.uuid().as_deref(),
        unix.as_deref(),
        hint.as_ref(),
        gio_volume_is_locked(volume),
    )
}

pub(crate) fn crypto_password_uuid_for_mount(mount: &gio::Mount) -> Option<String> {
    if let Some(volume) = mount.volume() {
        return crypto_password_uuid_for_volume(&volume);
    }
    let unix = mount
        .drive()
        .and_then(|drive| drive.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str()));
    let hint = unix
        .as_deref()
        .map(devices::block_crypto_hint)
        .unwrap_or_default();
    devices::crypto_password_uuid(None, unix.as_deref(), hint.as_ref(), false)
}




pub(crate) fn password_stop_drive(drive: Option<gio::Drive>) -> Option<gio::Drive> {
    drive.filter(|drive| {
        drive.start_stop_type() == gio::DriveStartStopType::Password && drive.can_stop()
    })
}

pub(crate) fn lock_cleartext_then_stop(
    mount: Option<gio::Mount>,
    drive: Option<gio::Drive>,
    parent: &gtk::Widget,
    browser: &Rc<Browser>,
    view: Option<BrowserView>,
    in_flight: &Rc<Cell<bool>>,
) {
    let key = match mount.as_ref() {
        Some(mount) => device_release::key_for_mount(mount),
        None => drive.as_ref().map(device_release::key_for_drive).unwrap_or(
            device_release::ReleaseKey {
                unix_device: None,
                uuid: None,
                mount_root: None,
                display_name: String::new(),
            },
        ),
    };
    let sync_root = mount.as_ref().and_then(|mount| mount.root().path());
    let away_root = mount.as_ref().map(gio::Mount::root);
    let in_flight_for_settle = in_flight.clone();
    let parent_for_stop = parent.clone();
    let key_for_stop = key.clone();
    let started = device_release::start_release(
        parent,
        Some(browser.clone()),
        view.as_ref(),
        key,
        device_release::ReleaseKind::Lock,
        false,
        sync_root,
        away_root,
        move || in_flight_for_settle.set(false),
        move |unmount_operation| async move {
            let unmount_result = match mount {
                Some(mount) => {
                    mount
                        .unmount_with_operation_future(
                            gio::MountUnmountFlags::NONE,
                            Some(&unmount_operation),
                        )
                        .await
                }
                None => Ok(()),
            };
            match unmount_result {
                Ok(()) => match drive {
                    Some(drive) => {
                        let stop_operation =
                            device_release::mount_operation(&parent_for_stop, &key_for_stop);
                        drive
                            .stop_future(gio::MountUnmountFlags::NONE, Some(&stop_operation))
                            .await
                    }
                    None => Ok(()),
                },
                Err(error) => Err(error),
            }
        },
    );
    if !started {
        in_flight.set(false);
    }
}


fn gio_icon_names(icon: &gio::Icon) -> Vec<String> {
    let mut names = Vec::new();
    collect_gio_icon_names(icon, &mut names);
    names
}

fn collect_gio_icon_names(icon: &gio::Icon, names: &mut Vec<String>) {
    if let Ok(themed) = icon.clone().downcast::<gio::ThemedIcon>() {
        names.extend(themed.names().iter().map(|name| name.to_string()));
        return;
    }
    if let Ok(emblemed) = icon.clone().downcast::<gio::EmblemedIcon>() {
        collect_gio_icon_names(&emblemed.icon(), names);
        for emblem in emblemed.emblems() {
            collect_gio_icon_names(&emblem.icon(), names);
        }
    }
}

fn gio_icons_are_encrypted(
    icon: &gio::Icon,
    symbolic: &gio::Icon,
    start_stop: Option<gio::DriveStartStopType>,
    unix_device: Option<&str>,
) -> bool {
    let mut names = gio_icon_names(symbolic);
    names.extend(gio_icon_names(icon));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let hint = unix_device
        .map(devices::block_crypto_hint)
        .unwrap_or_default();
    devices::is_encrypted_device(&names, start_stop, hint.as_ref())
}

pub(super) fn gio_volume_unix_device(volume: &gio::Volume) -> Option<glib::GString> {
    volume.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
}

pub(crate) fn gio_volume_is_encrypted(volume: &gio::Volume) -> bool {
    gio_icons_are_encrypted(
        &volume.icon(),
        &volume.symbolic_icon(),
        volume.drive().map(|drive| drive.start_stop_type()),
        gio_volume_unix_device(volume).as_deref(),
    )
}

fn gio_volume_is_locked(volume: &gio::Volume) -> bool {
    let mut names = gio_icon_names(&volume.symbolic_icon());
    names.extend(gio_icon_names(&volume.icon()));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    devices::encrypted_device_is_locked(&names, volume.get_mount().is_some())
}

pub(crate) fn gio_mount_is_encrypted(mount: &gio::Mount) -> bool {
    if let Some(volume) = mount.volume() {
        return gio_volume_is_encrypted(&volume);
    }
    gio_icons_are_encrypted(
        &mount.icon(),
        &mount.symbolic_icon(),
        mount.drive().map(|drive| drive.start_stop_type()),
        mount
            .drive()
            .and_then(|drive| drive.identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str()))
            .as_deref(),
    )
}

fn gio_volume_identity(volume: &gio::Volume) -> Option<String> {
    devices::device_identity(
        volume
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
            .as_deref(),
        volume.uuid().as_deref(),
        volume
            .drive()
            .and_then(|drive| gio_drive_identity(&drive))
            .as_deref(),
    )
}

fn gio_drive_identity(drive: &gio::Drive) -> Option<String> {
    devices::device_identity(
        drive
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
            .as_deref(),
        drive
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UUID.as_str())
            .as_deref(),
        None,
    )
}

pub(crate) fn orphaned_password_drives(volumes: &[gio::Volume], drives: &[gio::Drive]) -> Vec<gio::Drive> {
    let identities: Vec<_> = volumes.iter().filter_map(gio_volume_identity).collect();
    drives
        .iter()
        .filter(|drive| {
            devices::password_drive_is_orphaned(
                drive.start_stop_type(),
                gio_drive_identity(drive).as_deref(),
                &identities,
                volumes
                    .iter()
                    .any(|volume| volume.drive().as_ref() == Some(*drive)),
            )
        })
        .cloned()
        .collect()
}













fn sync_keycap(keycap: &gtk::Label, visible: bool) {
    if let Some(icon) = keycap.prev_sibling() {
        if visible && icon.is_visible() {
            // Matching the icon's width keeps the place name from shifting.
            keycap.set_width_request(icon.width());
        }
        icon.set_visible(!visible);
    }
    keycap.set_visible(visible);
}







pub(crate) fn home_directory() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}




