// SPDX-License-Identifier: MIT

use gtk::{glib, prelude::*};
use std::{cell::RefCell, collections::HashSet, rc::Rc};

use crate::ui::keys::{self, bindings::ESCAPE_ANY};

struct Target {
    id: &'static str,
    page: &'static str,
    title: &'static str,
    aliases: &'static str,
}

const TARGETS: &[Target] = &[
    Target {
        id: "cloud-sidebar",
        page: "cloud",
        title: "Show Cloud Storage in sidebar",
        aliases: "cloud accounts drive dropbox onedrive storage sidebar",
    },
    Target {
        id: "online-accounts",
        page: "cloud",
        title: "Online Accounts",
        aliases: "cloud account authentication sign in storage settings",
    },
    Target {
        id: "refresh-connections",
        page: "cloud",
        title: "Refresh connections",
        aliases: "cloud connection status mount cache bandwidth login",
    },
    Target {
        id: "preview-style",
        page: "general",
        title: "Preview style",
        aliases: "popup panel floating quick look",
    },
    Target {
        id: "language",
        page: "general",
        title: "Language",
        aliases: "locale translation auto detect restart English Français Deutsch Español 日本語 Português 한국어 Tiếng Việt Italiano Русский 简体中文",
    },
    Target {
        id: "custom-actions",
        page: "actions",
        title: "Custom actions",
        aliases: "scripts context menu commands automation python bash plugin extension",
    },
    Target {
        id: "new-action",
        page: "actions",
        title: "New custom action",
        aliases: "create script context menu command",
    },
    Target {
        id: "import-action",
        page: "actions",
        title: "Import custom action",
        aliases: "share copy actions folder",
    },
    Target {
        id: "thumbnail-workers",
        page: "general",
        title: "Thumbnail workers",
        aliases: "performance parallel concurrency cpu memory sandbox decoder pool",
    },
    Target {
        id: "default-directory",
        page: "general",
        title: "Default directory",
        aliases: "startup launch home folder reset",
    },
    Target {
        id: "restore-tabs",
        page: "general",
        title: "Restore open tabs",
        aliases: "startup launch session reopen tabs",
    },
    Target {
        id: "peeking",
        page: "general",
        title: "Folder peeking",
        aliases: "browsing preview folders pane",
    },
    Target {
        id: "previews",
        page: "general",
        title: "Single-click file previews",
        aliases: "browsing quick preview selecting supported files",
    },
    Target {
        id: "preview-autoplay",
        page: "general",
        title: "Autoplay media previews",
        aliases: "browsing video audio gif playback paused sound",
    },
    Target {
        id: "render-documents",
        page: "general",
        title: "Render documents by default",
        aliases: "browsing preview markdown md html rendered source view",
    },
    Target {
        id: "arrow-scope",
        page: "general",
        title: "Keep arrows in file list",
        aliases: "browsing keyboard navigation focus sidebar toolbar",
    },
    Target {
        id: "columns-sizes",
        page: "general",
        title: "Show sizes in Columns view",
        aliases: "columns miller size sizes folders count counts items metadata filenames",
    },
    Target {
        id: "columns-mirror",
        page: "general",
        title: "Mirror columns selection",
        aliases: "browsing columns keyboard selection next pane folders",
    },
    Target {
        id: "tenxer",
        page: "general",
        title: "10xer mode",
        aliases: "browsing keyboard modal prompts footer commands experimental",
    },
    Target {
        id: "hints",
        page: "general",
        title: "Show %{reference_keys} Shortcuts button",
        aliases: "browsing keyboard keybindings hints shortcuts reference help footer",
    },
    Target {
        id: "folder-views",
        page: "general",
        title: "Remember sort and icon size per folder",
        aliases: "browsing per folder sort order thumbnail size view state",
    },
    Target {
        id: "default-sort",
        page: "general",
        title: "Default sort",
        aliases: "browsing sort order name size modified type ascending descending",
    },
    Target {
        id: "forget-folder-views",
        page: "general",
        title: "Forget all folder settings",
        aliases: "browsing reset clear per folder sort thumbnail size",
    },
    Target {
        id: "type-search",
        page: "general",
        title: "Type to search",
        aliases: "filter keyboard typing pane",
    },
    Target {
        id: "subfolders",
        page: "general",
        title: "Include subfolders",
        aliases: "filter search recursive nested directories",
    },
    Target {
        id: "search-results",
        page: "general",
        title: "Open search results directly",
        aliases: "launch files quick preview",
    },
    Target {
        id: "search-time-limit",
        page: "general",
        title: "Search time limit",
        aliases: "search filter indexing walk timeout seconds minutes slow network mount partial results",
    },
    Target {
        id: "search-entry-limit",
        page: "general",
        title: "Search entry limit",
        aliases: "search filter indexing walk maximum files folders entries memory partial results",
    },
    Target {
        id: "search-exclusions",
        page: "general",
        title: "Global search exclusions",
        aliases: "search filter exclusions exclude folders directories ignore venv prune",
    },
    Target {
        id: "sidebar-places",
        page: "general",
        title: "Items shown in sidebar",
        aliases: "sidebar places hide show home trash network recent shares desktop documents downloads music pictures videos folder",
    },
    Target {
        id: "opening",
        page: "general",
        title: "Opening items",
        aliases: "single double click activation columns icons list files folders",
    },
    Target {
        id: "transfers",
        page: "general",
        title: "Drag & drop to another device",
        aliases: "file transfers copy move ask drive share cross volume",
    },
    Target {
        id: "open-after-drop",
        page: "general",
        title: "Open folder after dropping files",
        aliases: "file transfers drag drop destination navigate reveal copy move",
    },
    Target {
        id: "watch-network-folders",
        page: "general",
        title: "Watch network folders",
        aliases: "performance network share nfs smb sftp remote changes check refresh reload monitor",
    },
    Target {
        id: "video",
        page: "general",
        title: "Hardware-accelerated video previews",
        aliases: "performance gpu thumbnails decoding backend automatic va api vulkan software",
    },
    Target {
        id: "window-minimize",
        page: "general",
        title: "Show minimize button",
        aliases: "window buttons header top bar titlebar controls hide",
    },
    Target {
        id: "window-maximize",
        page: "general",
        title: "Show maximize button",
        aliases: "window buttons header top bar titlebar controls restore hide",
    },
    Target {
        id: "window-close",
        page: "general",
        title: "Show close button",
        aliases: "window buttons header top bar titlebar controls hide",
    },
    Target {
        id: "desktop",
        page: "general",
        title: "Desktop integration",
        aliases: "configure portal file chooser open save dialog default file manager encrypted udiskie",
    },
    Target {
        id: "udiskie-unlock",
        page: "general",
        title: "Unlock encrypted volumes",
        aliases: "encrypted luks password udiskie automount usb volume desktop integration omarchy",
    },
    Target {
        id: "omarchy",
        page: "theme",
        title: "Follow Omarchy",
        aliases: "appearance system theme quattro",
    },
    Target {
        id: "omarchy-variant",
        page: "theme",
        title: "Omarchy variant",
        aliases: "appearance system theme quattro palette original normal darker dark high contrast brightness",
    },
    Target {
        id: "current-theme",
        page: "theme",
        title: "Theme",
        aliases: "appearance palette colors selected theme edit file current",
    },
    Target {
        id: "date-format",
        page: "general",
        title: "Modified date format",
        aliases: "date time format relative iso 8601 long absolute modified timestamp clock appearance",
    },
    Target {
        id: "show-thumbnails",
        page: "theme",
        title: "Show thumbnails",
        aliases: "appearance images pictures photos videos documents thumbnails icons previews disable",
    },
    Target {
        id: "colored-sidebar-icons",
        page: "theme",
        title: "Colored sidebar icons",
        aliases: "appearance sidebar places pinned icons colour color muted grey accent",
    },
    Target {
        id: "file-icon-colors",
        page: "theme",
        title: "File icon colors",
        aliases: "appearance file type icons brand palette accent theme monochrome catppuccin",
    },
    Target {
        id: "motion",
        page: "theme",
        title: "Reduce motion",
        aliases: "appearance disable animations",
    },
    Target {
        id: "renderer",
        page: "theme",
        title: "Interface renderer",
        aliases: "appearance graphics cairo gtk gpu text artifacts performance restart",
    },
    Target {
        id: "keyboard",
        page: "keyboard",
        title: "Keyboard",
        aliases: "keys shortcuts keybindings hotkeys",
    },
    Target {
        id: "diagnostics",
        page: "diagnostics",
        title: "Run diagnostics",
        aliases: "troubleshoot troubleshooting problems health check dependencies missing packages report bug sandbox bubblewrap ffmpeg gstreamer gvfs sftp smb udisks portal",
    },
    Target {
        id: "version",
        page: "about",
        title: "Version information",
        aliases: "files nyx build commit toolkit gtk copy author",
    },
    Target {
        id: "website",
        page: "about",
        title: "Website",
        aliases: "links nyx files home",
    },
    Target {
        id: "source",
        page: "about",
        title: "Source code",
        aliases: "links github repository",
    },
    Target {
        id: "issue",
        page: "about",
        title: "Report an issue",
        aliases: "links bug feedback",
    },
    Target {
        id: "license",
        page: "about",
        title: "License",
        aliases: "links mit copyright",
    },
];

pub(super) fn tag(widget: &impl IsA<gtk::Widget>, title: &str) {
    if let Some(target) = TARGETS.iter().find(|target| target.title == title) {
        widget.set_widget_name(&format!("settings-search-{}", target.id));
    }
}













fn children(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut children = Vec::new();
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        children.push(widget);
    }
    children
}




pub(super) fn set_compact(navigation: &gtk::Box, compact: bool) {
    let children = children(navigation.upcast_ref());
    let field = children
        .iter()
        .find(|child| child.has_css_class("settings-global-search"));
    let button = children
        .iter()
        .find(|child| child.has_css_class("settings-global-search-compact"));
    let (Some(field), Some(button)) = (field, button) else {
        return;
    };
    let transfer_focus = navigation
        .root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus.is_ancestor(if compact { field } else { button }));
    field.set_visible(!compact);
    button.set_visible(compact);
    if transfer_focus {
        let target = if compact {
            button.clone()
        } else {
            field.first_child().expect("search entry")
        }
        .downgrade();
        glib::idle_add_local_once(move || {
            if let Some(target) = target.upgrade()
                && target.is_mapped()
            {
                if let Some(button) = target.downcast_ref::<gtk::MenuButton>() {
                    button.popup();
                } else {
                    target.grab_focus();
                }
            }
        });
    }
}





#[cfg(test)]
mod tests;
