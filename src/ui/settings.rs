// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    process::Command,
};

use gtk::{gio, glib, prelude::*, subclass::prelude::*};




pub(crate) mod actions;
pub(crate) mod diagnostics;
pub(crate) mod exclusions;
pub(crate) mod general;
mod keyboard;
mod install_command;
mod search;
mod theme;



pub struct ResponsiveActivationRow {
    row: gtk::Box,
    options: Vec<gtk::Box>,
}

const DIALOG_WIDTH: i32 = 1400;
const DIALOG_HEIGHT: i32 = 1024;
const DIALOG_MARGIN: i32 = 24;
const COMPACT_NAVIGATION_BREAKPOINT: i32 = 900;
// Reflow the content before collapsing navigation: desktop toolbars need more room.
const COMPACT_CONTENT_BREAKPOINT: i32 = 1250;
const MIN_SIDE_BY_SIDE_ACTIVATION_WIDTH: i32 = 620;
const STACK_ACTIVATION_OPTIONS_BREAKPOINT: i32 = 350;
const STACK_TEXT_SIZE_BREAKPOINT: i32 = 600;
const STACK_EXCLUSION_INPUT_BREAKPOINT: i32 = 1000;

mod responsive_bin {
    use super::*;

    #[derive(Default)]
    pub struct ResponsiveBin {
        pub compact_navigation: Cell<bool>,
        pub compact_content: Cell<bool>,
        pub typography_scale: Cell<f64>,
        // Matrix widths below which the translated choices were seen squeezed: side by side,
        // or with each label beside its control.
        pub activation_side_by_side_min: Cell<i32>,
        pub activation_inline_options_min: Cell<i32>,
        pub navigation: RefCell<Option<gtk::Box>>,
        pub navigation_heading: RefCell<Option<gtk::Label>>,
        pub navigation_labels: RefCell<Vec<gtk::Label>>,
        pub navigation_contents: RefCell<Vec<gtk::Box>>,
        pub responsive_flows: RefCell<Vec<(gtk::FlowBox, u32)>>,
        pub responsive_actions: RefCell<Vec<(gtk::Box, gtk::Button)>>,
        pub responsive_setting_rows: RefCell<Vec<gtk::Box>>,
        pub responsive_activation_rows: RefCell<Vec<ResponsiveActivationRow>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ResponsiveBin {
        const NAME: &'static str = "StrataSettingsResponsiveBin";
        type Type = super::ResponsiveBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for ResponsiveBin {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for ResponsiveBin {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let natural = match orientation {
                gtk::Orientation::Horizontal => DIALOG_WIDTH + DIALOG_MARGIN * 2,
                gtk::Orientation::Vertical => DIALOG_HEIGHT + DIALOG_MARGIN * 2,
                _ => unreachable!("GTK orientations are horizontal or vertical"),
            };
            (1, natural, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(child) = self.obj().first_child() else {
                return;
            };
            let (child_width, child_height) = responsive_dialog_size(width, height);
            let logical_width = f64::from(child_width) / self.typography_scale.get().max(0.1);
            let compact_content = logical_width < f64::from(COMPACT_CONTENT_BREAKPOINT);
            let compact = uses_compact_navigation(
                (f64::from(child_width) / self.typography_scale.get().max(0.1)) as i32,
            );
            if self.compact_navigation.replace(compact) != compact {
                if let Some(navigation) = self.navigation.borrow().as_ref() {
                    search::set_compact(navigation, compact);
                    if compact {
                        navigation.add_css_class("compact");
                    } else {
                        navigation.remove_css_class("compact");
                    }
                }
                if let Some(heading) = self.navigation_heading.borrow().as_ref() {
                    heading.set_visible(!compact);
                }
                for label in self.navigation_labels.borrow().iter() {
                    label.set_visible(!compact);
                }
                for content in self.navigation_contents.borrow().iter() {
                    content.set_halign(if compact {
                        gtk::Align::Center
                    } else {
                        gtk::Align::Fill
                    });
                }
            }
            if self.compact_content.replace(compact_content) != compact_content {
                let compact = compact_content;
                for (flow, expanded_columns) in self.responsive_flows.borrow().iter() {
                    flow.set_max_children_per_line(if compact { 1 } else { *expanded_columns });
                }
                for (row, action) in self.responsive_actions.borrow().iter() {
                    row.set_orientation(if compact {
                        gtk::Orientation::Vertical
                    } else {
                        gtk::Orientation::Horizontal
                    });
                    action.set_halign(gtk::Align::Fill);
                }
                for row in self.responsive_setting_rows.borrow().iter() {
                    row.set_orientation(if compact {
                        gtk::Orientation::Vertical
                    } else {
                        gtk::Orientation::Horizontal
                    });
                    row.set_spacing(if compact { 8 } else { 16 });
                }
            }
            let available_activation_width = child_width - if compact { 100 } else { 330 };
            let scaled_activation_width = MIN_SIDE_BY_SIDE_ACTIVATION_WIDTH as f64
                + (self.typography_scale.get() - 1.0).max(0.0) * 320.0;
            let matrix_width = activation_matrix_width(&self.responsive_activation_rows.borrow());
            let activation_compact = f64::from(available_activation_width)
                < scaled_activation_width
                || matrix_width < self.activation_side_by_side_min.get();
            let stack_activation_options = available_activation_width
                < STACK_ACTIVATION_OPTIONS_BREAKPOINT
                || matrix_width < self.activation_inline_options_min.get();
            for responsive_row in self.responsive_activation_rows.borrow().iter() {
                responsive_row.row.set_orientation(if activation_compact {
                    gtk::Orientation::Vertical
                } else {
                    gtk::Orientation::Horizontal
                });
                responsive_row
                    .row
                    .set_spacing(if activation_compact { 4 } else { 24 });
                for option in &responsive_row.options {
                    option.set_orientation(if stack_activation_options {
                        gtk::Orientation::Vertical
                    } else {
                        gtk::Orientation::Horizontal
                    });
                    option.set_spacing(if stack_activation_options { 2 } else { 6 });
                    option.set_halign(if activation_compact && !stack_activation_options {
                        gtk::Align::End
                    } else {
                        gtk::Align::Fill
                    });
                }
                if activation_compact {
                    responsive_row.row.add_css_class("compact");
                } else {
                    responsive_row.row.remove_css_class("compact");
                }
            }
            reflow_settings(
                &child,
                compact_content,
                activation_compact,
                logical_width < f64::from(STACK_TEXT_SIZE_BREAKPOINT),
                logical_width < f64::from(STACK_EXCLUSION_INPUT_BREAKPOINT),
            );
            let x = ((width - child_width) / 2) as f32;
            let y = ((height - child_height) / 2) as f32;
            let transform = gtk::gsk::Transform::new().translate(&gtk::graphene::Point::new(x, y));
            child.allocate(child_width, child_height, baseline, Some(transform));
            self.stack_squeezed_activation_choices(activation_compact, stack_activation_options);
        }
    }

    impl ResponsiveBin {
        /// Long translated choices can need more room than the fixed breakpoints allow; once
        /// a choice gets less than its natural width, stack the matrix further at this width.
        fn stack_squeezed_activation_choices(&self, compact: bool, stacked_options: bool) {
            if compact && stacked_options {
                return;
            }
            let rows = self.responsive_activation_rows.borrow();
            let squeezed = rows.iter().filter(|row| row.row.is_mapped()).any(|row| {
                row.options
                    .iter()
                    .filter_map(|option| option.last_child())
                    .any(|control| choice_label_wraps(&control))
            });
            if !squeezed {
                return;
            }
            let threshold = activation_matrix_width(&rows) + 1;
            if compact {
                self.activation_inline_options_min.set(threshold);
            } else {
                self.activation_side_by_side_min.set(threshold);
            }
            let bin = self.obj().downgrade();
            glib::idle_add_local_once(move || {
                if let Some(bin) = bin.upgrade() {
                    bin.queue_allocate();
                }
            });
        }
    }
}

glib::wrapper! {
    pub struct ResponsiveBin(ObjectSubclass<responsive_bin::ResponsiveBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}


/// Whether a segmented control gives any choice less than its one-line width.
fn choice_label_wraps(control: &gtk::Widget) -> bool {
    let mut choice = control.first_child();
    while let Some(button) = choice {
        choice = button.next_sibling();
        if let Some(label) = button.downcast_ref::<gtk::Button>().and_then(|b| b.child())
            && label.width() < label.measure(gtk::Orientation::Horizontal, -1).1
        {
            return true;
        }
    }
    false
}

fn activation_matrix_width(rows: &[ResponsiveActivationRow]) -> i32 {
    rows.first()
        .and_then(|row| row.row.parent())
        .map_or(0, |matrix| matrix.width())
}

fn reflow_settings(
    widget: &gtk::Widget,
    compact: bool,
    activation_compact: bool,
    stack_text_size: bool,
    stack_exclusion_input: bool,
) {
    if widget.has_css_class("settings-dialog") {
        if compact {
            widget.add_css_class("compact");
        } else {
            widget.remove_css_class("compact");
        }
    }
    if let Some(row) = widget.downcast_ref::<gtk::Box>()
        && [
            "settings-option",
            "settings-library-toolbar",
            "about-identity",
            "theme-library-footer",
            "settings-inline-description",
            "settings-update-summary",
        ]
        .iter()
        .any(|class| row.has_css_class(class))
    {
        let switch_row = row
            .last_child()
            .is_some_and(|child| child.is::<gtk::Switch>());
        // Short numeric controls can stay beside wrapping copy after other rows stack.
        let stack_row = if row.has_css_class("settings-renderer-row")
            || row.has_css_class("settings-accent-row")
            || row.has_css_class("settings-exclusions-row")
        {
            true
        } else if row.has_css_class("settings-text-size-row") {
            stack_text_size
        } else {
            compact
        };
        let orientation = if stack_row && !switch_row {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        };
        // Release notes and sidebar chips always stay below their row's copy.
        if !row.has_css_class("settings-update-status")
            && !row.has_css_class("settings-sidebar-places")
            && row.orientation() != orientation
        {
            row.set_orientation(orientation);
        }
    }
    if widget.has_css_class("settings-integration-actions")
        && let Some(actions) = widget.downcast_ref::<gtk::Box>()
    {
        actions.set_orientation(if compact {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
    }
    if widget.has_css_class("theme-appearance-filter")
        || widget.has_css_class("settings-integration-actions")
    {
        widget.set_halign(if compact {
            gtk::Align::Fill
        } else {
            gtk::Align::End
        });
        widget.set_hexpand(compact);
        let mut child = widget.first_child();
        while let Some(button) = child {
            child = button.next_sibling();
            button.set_hexpand(compact);
        }
    }
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && (label.has_css_class("settings-nowrap") || label.has_css_class("menu-heading"))
    {
        label.set_wrap(compact && !label.has_css_class("settings-keycap"));
    }
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && !widget.is::<gtk::ToggleButton>()
        && let Some(label) = button.child().and_downcast::<gtk::Label>()
    {
        label.set_wrap(compact);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    }
    // Keep the exclusion placeholder readable instead of squeezing it beside translated buttons.
    if widget.has_css_class("settings-exclusions-input")
        && let Some(row) = widget.downcast_ref::<gtk::Box>()
    {
        let orientation = if stack_exclusion_input {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        };
        if row.orientation() != orientation {
            row.set_orientation(orientation);
        }
    }
    if widget.has_css_class("activation-header") {
        widget.set_visible(!activation_compact);
    }
    if widget.has_css_class("activation-inline-label") {
        widget.set_visible(activation_compact);
    }
    // Beside the choices, keep long single-word view names whole; stacked, they have the full width.
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && label.has_css_class("click-activation-title")
    {
        let mode = if activation_compact {
            gtk::pango::WrapMode::WordChar
        } else {
            gtk::pango::WrapMode::Word
        };
        if label.wrap_mode() != mode {
            label.set_wrap_mode(mode);
        }
    }
    let mut child = widget.first_child();
    while let Some(next) = child {
        child = next.next_sibling();
        reflow_settings(
            &next,
            compact,
            activation_compact,
            stack_text_size,
            stack_exclusion_input,
        );
    }
}

fn responsive_dialog_size(width: i32, height: i32) -> (i32, i32) {
    (
        DIALOG_WIDTH.min((width - DIALOG_MARGIN * 2).max(1)),
        DIALOG_HEIGHT.min((height - DIALOG_MARGIN * 2).max(1)),
    )
}

fn uses_compact_navigation(dialog_width: i32) -> bool {
    dialog_width < COMPACT_NAVIGATION_BREAKPOINT
}






fn process_start_time(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm (field 2) may contain spaces or parentheses; the remaining fields
    // begin after its final closing parenthesis. starttime is field 22.
    stat.rsplit_once(") ")?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}

fn restart_waiter(current_exe: &std::path::Path, parent_pid: u32) -> Option<Command> {
    use std::{os::unix::process::CommandExt, process::Stdio};

    crate::util::ui_thread::assert_off();
    let mut command = crate::trusted_command::command("sh").ok()?;
    let sleep = crate::trusted_command::resolve("sleep").ok()?;
    let start_time = process_start_time(parent_pid).unwrap_or_default();
    command
        .args([
            "-c",
            "pid=$1; binary=$2; sleeper=$3; born=$4; while kill -0 \"$pid\" 2>/dev/null && [ -r \"/proc/$pid/stat\" ]; do IFS= read -r stat < \"/proc/$pid/stat\" || break; rest=${stat##*) }; set -- $rest; shift 19; [ \"${1:-}\" = \"$born\" ] || break; \"$sleeper\" 0.1; done; \"$sleeper\" 0.5; exec \"$binary\"",
            "files-restart",
        ])
        .arg(parent_pid.to_string())
        .arg(current_exe)
        .arg(sleep)
        .arg(start_time)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if std::env::var_os(crate::CAIRO_SELECTED_BY_STRATA).as_deref()
        == Some(std::ffi::OsStr::new("1"))
        && std::env::var_os("GSK_RENDERER").as_deref() == Some(std::ffi::OsStr::new("cairo"))
    {
        // The next process must read the saved renderer, not inherit our default.
        command.env_remove("GSK_RENDERER");
        command.env_remove(crate::CAIRO_SELECTED_BY_STRATA);
    }
    Some(command)
}

pub(crate) fn restart(application: Option<&gtk::Application>) {
    restart_with(application, start_restart_waiter);
}

fn start_restart_waiter() -> Option<std::process::Child> {
    let current_exe = crate::services::installed_executable().ok()?;
    // Wait for this process to exit completely before relaunching. A fixed
    // delay could overlap the old and new GTK/Wayland clients and rapidly hand
    // keyboard focus through an underlying terminal. Besides re-activating the
    // old GApplication instance, that exposed a Foot/libxkbcommon crash on
    // affected systems. Detach the waiter from inherited terminal streams and
    // put it in its own process group so restarting cannot disturb the
    // terminal that launched Files.
    restart_waiter(&current_exe, std::process::id())?
        .spawn()
        .ok()
}

thread_local! {
    static RESTART_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
}

/// A closed window may no longer have an application; never exit the process in that case.
fn restart_with(
    application: Option<&gtk::Application>,
    start: impl FnOnce() -> Option<std::process::Child> + Send + 'static,
) {
    let Some(application) = application.cloned() else {
        return;
    };
    if RESTART_IN_FLIGHT.get() || restart_blocker_shown(&application) {
        return;
    }
    RESTART_IN_FLIGHT.set(true);
    glib::spawn_future_local(async move {
        let started = gio::spawn_blocking(start).await;
        RESTART_IN_FLIGHT.set(false);
        let Ok(Some(mut waiter)) = started else {
            return;
        };
        // Do not leave a waiter behind when an unregistered close handler refuses.
        if !close_windows_for_restart(&application) {
            gio::spawn_blocking(move || {
                let _ = waiter.kill();
                let _ = waiter.wait();
            });
            return;
        }
        application.quit();
    });
}

/// Checks every window before closing any, so a refusal cannot leave some closed.
fn restart_blocker_shown(application: &gtk::Application) -> bool {
    let Some((window, blocker)) = crate::ui::close_guard::application_blocker(application) else {
        return false;
    };
    window.present();
    blocker.show(&window);
    true
}

fn close_windows_for_restart(application: &gtk::Application) -> bool {
    if restart_blocker_shown(application) {
        return false;
    }
    for window in application.windows() {
        window.close();
    }
    application.windows().is_empty()
}




/// Translates Settings copy, rendering the shortcuts it names.
pub(crate) fn translated(message: &str) -> String {
    translated_in(&rust_i18n::locale(), message)
}

fn translated_in(locale: &str, message: &str) -> String {
    use super::keys::{
        Keys::One,
        bindings::{REFERENCE, SIDEBAR_FOCUS, TENXER_MODE},
        label_in,
    };
    rust_i18n::t!(
        message,
        locale = locale,
        reference_keys = label_in(locale, &One(REFERENCE)),
        sidebar_focus_keys = label_in(locale, &One(SIDEBAR_FOCUS)),
        tenxer_mode_keys = label_in(locale, &One(TENXER_MODE))
    )
    .into_owned()
}

fn settings_option(title: &str, description: &str, active: bool) -> (gtk::Box, gtk::Switch) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    search::tag(&row, title);
    row.add_css_class("settings-option");
    let copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
    copy.set_hexpand(true);
    copy.set_valign(gtk::Align::Center);
    let title_label = gtk::Label::new(Some(&translated(title)));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    title_label.add_css_class("settings-option-title");
    let description_label = gtk::Label::new(Some(&translated(description)));
    description_label.set_xalign(0.0);
    description_label.set_wrap(true);
    description_label.add_css_class("settings-option-description");
    description_label.set_visible(!description.is_empty());
    copy.append(&title_label);
    copy.append(&description_label);
    let toggle = gtk::Switch::builder()
        .active(active)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    super::accessibility::set_own_label(&toggle, &translated(title));
    super::accessibility::set_description(&toggle, Some(&translated(description)));
    row.append(&copy);
    row.append(&toggle);
    (row, toggle)
}






