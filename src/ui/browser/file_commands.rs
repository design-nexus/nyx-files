// SPDX-License-Identifier: MIT

use std::{path::PathBuf, rc::Rc};

use gtk::{glib, prelude::*};

use super::{
    BrowserView, PinStatus,
    clipboard::{self},
    desktop::{launch_terminal},
    paths::{is_trash_location},
};
use crate::{
    adapters::gio_file_for_location,
    model::{FileEntry, Location, SortDirection, SortKey},
    services::{ActionHandle, ArchiveFormat, InvocationSource},
};



#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyboardRefocus {
    Sort(usize),
    Rename(crate::services::OperationRequestId),
}






impl BrowserView {










}




impl BrowserView {

}

impl super::ViewState {

}

impl BrowserView {





}

impl BrowserView {








}
