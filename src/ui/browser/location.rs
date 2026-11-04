// SPDX-License-Identifier: MIT

use crate::model::Location;
use crate::ui::blur::BlurBin;
use crate::ui::browser::{BrowserView, ViewState};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;


// Long crumbs middle-elide past this cap; the scroller handles deeper paths.


pub(super) struct UnlockProgressView {
    layer: gtk::Box,
    overlay: gtk::Overlay,
    blurred_root: Option<BlurBin>,
}

pub(super) struct UnlockProgressSlot {
    keys: DeviceKeys,
    view: Option<UnlockProgressView>,
    pending: Option<glib::SourceId>,
    dismissed: bool,
    in_flight: bool,
}

pub(super) mod completion;








fn password_field_label(message: &str) -> &'static str {
    if message.to_ascii_lowercase().contains("passphrase") {
        "Passphrase"
    } else {
        "Password"
    }
}







fn append_authentication_field(fields: &gtk::Box, label_text: &str, field: &impl IsA<gtk::Widget>) {
    let group = gtk::Box::new(gtk::Orientation::Vertical, 5);
    group.append(&gtk::Label::new(Some(&crate::i18n::tr(label_text))));
    group.append(field);
    fields.append(&group);
}





/// How a typed location a dialog waits on turned out.
pub(in crate::ui) enum ConnectOutcome {
    Opened(Location),
    /// The message to show, or `None` when the user cancelled authentication.
    Failed(Option<String>),
}

/// A typed location a dialog waits on until it opens or fails.
pub(in crate::ui) struct PendingConnect {
    /// Unknown until the typed input resolves; remote targets open asynchronously.
    target: RefCell<Option<Location>>,
    cancellable: gio::Cancellable,
    finished: Box<dyn Fn(ConnectOutcome)>,
}


#[derive(Clone)]
pub(super) struct MountCredentials {
    anonymous: bool,
    username: String,
    domain: String,
    password: String,
    save: gio::PasswordSave,
}

impl MountCredentials {
}























#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DeviceKeys {
    volume_tokens: Vec<String>,
    drive_tokens: Vec<String>,
    volume_object: Option<glib::Object>,
    drive_object: Option<glib::Object>,
}

impl DeviceKeys {


}























impl BrowserView {


}

impl ViewState {





































}


