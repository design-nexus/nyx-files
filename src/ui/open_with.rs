// SPDX-License-Identifier: MIT

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::OnceLock,
};

use gtk::{gio, glib, prelude::*};

use super::recent_apps;

/// Inside a Flatpak sandbox GIO only sees the runtime's desktop entries, so
/// files are handed to the host through the OpenURI portal instead.
pub(crate) fn uses_portal() -> bool {
    static FLATPAK: OnceLock<bool> = OnceLock::new();
    *FLATPAK.get_or_init(|| {
        #[cfg(not(test))]
        crate::util::ui_thread::assert_off();
        std::path::Path::new("/.flatpak-info").exists()
    })
}

/// Opens each file with the host's default handler, or lets the desktop's
/// own chooser pick one when `always_ask` is set. Files open one after
/// another so choosers do not stack; dismissing one stops the rest.
pub(crate) fn launch_with_portal(
    parent: &impl IsA<gtk::Widget>,
    files: Vec<gio::File>,
    always_ask: bool,
) {
    let window = parent
        .as_ref()
        .root()
        .and_downcast::<gtk::Window>()
        .map(|window| window.downgrade());
    let parent = parent.as_ref().downgrade();
    glib::MainContext::default().spawn_local(async move {
        for file in files {
            let window = window.as_ref().and_then(glib::WeakRef::upgrade);
            let launcher = gtk::FileLauncher::new(Some(&file));
            launcher.set_always_ask(always_ask);
            let Err(error) = launcher.launch_future(window.as_ref()).await else {
                continue;
            };
            if !error.matches(gtk::DialogError::Dismissed)
                && !error.matches(gio::IOErrorEnum::Cancelled)
                && let Some(parent) = parent.upgrade()
            {
                super::modal::show_error_dialog(
                    &parent,
                    &crate::i18n::tr("Unable to open file"),
                    error.message(),
                );
            }
            return;
        }
    });
}

fn categorized_apps(
    content_type: &str,
    requires_uris: bool,
) -> (Vec<gio::AppInfo>, Vec<gio::AppInfo>) {
    let default = gio::AppInfo::default_for_type(content_type, requires_uris);
    let recommended = filter_apps(
        gio::AppInfo::all_for_type(content_type),
        default,
        requires_uris,
    );
    let other = filter_other_apps(gio::AppInfo::all(), &recommended, requires_uris);
    (recommended, other)
}

// Non-native GVfs files can still provide FUSE paths for %f/%F handlers.
pub(super) fn requires_uri_handlers(files: &[gio::File]) -> bool {
    files
        .iter()
        .any(|file| path_requires_uri_handlers(file.path().as_deref()))
}

fn path_requires_uri_handlers(path: Option<&std::path::Path>) -> bool {
    path.is_none()
}

fn app_is_candidate(app: &gio::AppInfo, requires_uris: bool) -> bool {
    app.should_show() && (!requires_uris || app.supports_uris())
}

fn filter_apps(
    apps: Vec<gio::AppInfo>,
    default: Option<gio::AppInfo>,
    requires_uris: bool,
) -> Vec<gio::AppInfo> {
    let mut apps = apps
        .into_iter()
        .filter(|app| app_is_candidate(app, requires_uris))
        .collect::<Vec<_>>();
    apps.sort_by_cached_key(|app| app.display_name().to_lowercase());
    let mut unique = Vec::with_capacity(apps.len());
    if let Some(default) = default.filter(|app| !requires_uris || app.supports_uris()) {
        unique.push(default);
    }
    for app in apps {
        if !unique.iter().any(|existing| existing.equal(&app)) {
            unique.push(app);
        }
    }
    unique
}

pub(super) fn filter_other_apps(
    apps: Vec<gio::AppInfo>,
    recommended: &[gio::AppInfo],
    requires_uris: bool,
) -> Vec<gio::AppInfo> {
    let mut apps = apps
        .into_iter()
        .filter(|app| {
            app_is_candidate(app, requires_uris) && !recommended.iter().any(|rec| rec.equal(app))
        })
        .collect::<Vec<_>>();
    apps.sort_by_cached_key(|app| app.display_name().to_lowercase());
    let mut unique: Vec<gio::AppInfo> = Vec::with_capacity(apps.len());
    for app in apps {
        if !unique.iter().any(|existing| existing.equal(&app)) {
            unique.push(app);
        }
    }
    unique
}

fn app_ids(apps: &[gio::AppInfo]) -> HashSet<String> {
    apps.iter()
        .filter_map(|app| app.id().map(|id| id.to_string()))
        .collect()
}

// For mixed types, only promote handlers shared by every type. Single-type
// history may also promote applications explicitly chosen from Other.
fn split_recent(
    history: &recent_apps::State,
    content_types: &[String],
    recommended: &mut Vec<gio::AppInfo>,
    other: &mut Vec<gio::AppInfo>,
) -> Vec<gio::AppInfo> {
    let ordered = history.recent_ids(content_types);
    if ordered.is_empty() {
        return Vec::new();
    }
    let mut eligible = app_ids(recommended);
    if content_types.len() <= 1 {
        eligible.extend(app_ids(other));
    }
    let mut wanted = recent_apps::select_recent(&ordered, &eligible);
    wanted.truncate(recent_apps::MAX_PER_TYPE);
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut by_id: HashMap<String, gio::AppInfo> = HashMap::new();
    for app in recommended.iter().chain(other.iter()) {
        if let Some(id) = app.id() {
            by_id.entry(id.to_string()).or_insert_with(|| app.clone());
        }
    }
    let recent: Vec<gio::AppInfo> = wanted.iter().filter_map(|id| by_id.remove(id)).collect();
    if recent.is_empty() {
        return Vec::new();
    }
    let recent_ids: HashSet<String> = wanted.into_iter().collect();
    for apps in [recommended, other] {
        apps.retain(|app| app.id().is_none_or(|id| !recent_ids.contains(id.as_str())));
    }
    recent
}

#[derive(Clone, Default)]
pub(crate) struct Choices {
    pub(crate) recent: Vec<gio::AppInfo>,
    pub(crate) recommended: Vec<gio::AppInfo>,
    pub(crate) other: Vec<gio::AppInfo>,
}

impl Choices {
    fn new(
        history: &recent_apps::State,
        content_types: &[String],
        mut recommended: Vec<gio::AppInfo>,
        mut other: Vec<gio::AppInfo>,
    ) -> Self {
        let recent = split_recent(history, content_types, &mut recommended, &mut other);
        Self {
            recent,
            recommended,
            other,
        }
    }

    fn is_empty(&self) -> bool {
        self.recent.is_empty() && self.recommended.is_empty() && self.other.is_empty()
    }
}

pub(crate) struct Applications {
    pub(crate) choices: Choices,
    pub(crate) default: Option<gio::AppInfo>,
    pub(crate) content_types: Vec<String>,
}

impl Applications {
    pub(crate) fn unavailable_reason(&self) -> Option<&'static str> {
        if uses_portal() || !self.choices.is_empty() {
            None
        } else if self.content_types.len() > 1 {
            Some("No application can open all selected file types")
        } else {
            Some("No compatible applications were found")
        }
    }
}

/// Looks up the type of every file, then the handlers they all share. An
/// unreadable file or broken link fails the whole lookup rather than letting
/// another item decide. `None` means `current` stopped holding first.
pub(crate) async fn resolve(
    files: &[gio::File],
    current: impl Fn() -> bool,
) -> Option<Result<Applications, &'static str>> {
    let mut content_types = Vec::<String>::new();
    for file in files {
        if !current() {
            return None;
        }
        let info = file
            .query_info_future(
                "standard::type,standard::content-type",
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await;
        if !current() {
            return None;
        }
        let Ok(info) = info else {
            return Some(Err("Unable to read the selected file type"));
        };
        if info.file_type() == gio::FileType::SymbolicLink {
            return Some(Err(
                "Broken symbolic links cannot be opened with an application",
            ));
        }
        let Some(next_type) = info.content_type().map(|value| value.to_string()) else {
            return Some(Err("Unable to determine the selected file type"));
        };
        if !content_types
            .iter()
            .any(|value| gio::content_type_equals(value, &next_type))
        {
            content_types.push(next_type);
        }
    }
    if !current() {
        return None;
    }
    let history = recent_apps::read().await.unwrap_or_default();
    if !current() {
        return None;
    }
    let requires_uris = requires_uri_handlers(files);
    let (recommended, other, default) = common_applications(&content_types, requires_uris);
    Some(Ok(Applications {
        choices: Choices::new(&history, &content_types, recommended, other),
        default,
        content_types,
    }))
}

fn common_applications(
    content_types: &[String],
    requires_uris: bool,
) -> (Vec<gio::AppInfo>, Vec<gio::AppInfo>, Option<gio::AppInfo>) {
    let Some(first) = content_types.first() else {
        return (vec![], vec![], None);
    };
    let (mut recommended, _) = categorized_apps(first, requires_uris);
    let mut default = gio::AppInfo::default_for_type(first, requires_uris);
    for content_type in &content_types[1..] {
        let (next_rec, _) = categorized_apps(content_type, requires_uris);
        recommended.retain(|app| next_rec.iter().any(|candidate| candidate.equal(app)));
        let next_default = gio::AppInfo::default_for_type(content_type, requires_uris);
        default = default.filter(|app| next_default.as_ref().is_some_and(|next| next.equal(app)));
    }
    let other = filter_other_apps(gio::AppInfo::all(), &recommended, requires_uris);
    (recommended, other, default)
}

pub(crate) fn launch(
    app: &gio::AppInfo,
    files: &[gio::File],
    content_types: &[String],
    context: Option<&impl IsA<gio::AppLaunchContext>>,
) -> Result<(), glib::Error> {
    launch_with_recent_registration(
        app,
        app,
        files,
        content_types,
        context,
        register_recent_file,
    )
}

pub(in crate::ui) fn launch_at(
    app: &gio::AppInfo,
    file: &gio::File,
    content_type: &str,
    position: Option<std::time::Duration>,
    context: Option<&impl IsA<gio::AppLaunchContext>>,
) -> Result<(), glib::Error> {
    let files = std::slice::from_ref(file);
    let content_types = [content_type.to_owned()];
    let timed = position
        .and_then(|position| {
            let exec = app.commandline()?;
            command_line_at(&exec.to_string_lossy(), position.as_secs_f64())
        })
        .and_then(|command_line| {
            let mut flags = if app.supports_uris() {
                gio::AppInfoCreateFlags::SUPPORTS_URIS
            } else {
                gio::AppInfoCreateFlags::NONE
            };
            if app
                .downcast_ref::<gio_unix::DesktopAppInfo>()
                .is_some_and(|desktop| desktop.boolean("StartupNotify"))
            {
                flags |= gio::AppInfoCreateFlags::SUPPORTS_STARTUP_NOTIFICATION;
            }
            gio::AppInfo::create_from_commandline(command_line, Some(app.name().as_str()), flags)
                .ok()
        });
    if let Some(timed) = timed
        && launch_with_recent_registration(
            app,
            &timed,
            files,
            &content_types,
            context,
            register_recent_file,
        )
        .is_ok()
    {
        return Ok(());
    }
    launch(app, files, &content_types, context)
}

/// Field codes and Flatpak forwarding markers must stay unquoted and mark
/// where the file arguments go.
fn is_placeholder(token: &str) -> bool {
    matches!(
        token,
        "%f" | "%F" | "%u" | "%U" | "%i" | "%c" | "%k" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m"
    ) || token.starts_with("@@")
}

fn basename(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

fn player_name(tokens: &[String]) -> Option<String> {
    let mut command = tokens.iter().peekable();
    let mut program = command.next()?;
    while basename(program) == "env" {
        program = command.find(|token| !token.contains('=') && !token.starts_with('-'))?;
    }
    if basename(program) == "flatpak" {
        if let Some(name) = tokens
            .iter()
            .find_map(|token| token.strip_prefix("--command="))
        {
            return Some(basename(name).to_owned());
        }
        return tokens
            .iter()
            .skip(1)
            .find(|token| {
                token.contains('.')
                    && !token.starts_with('-')
                    && !token.contains('/')
                    && !is_placeholder(token)
            })
            .cloned();
    }
    Some(basename(program).to_owned())
}

fn start_arguments(player: &str, seconds: f64) -> Option<Vec<String>> {
    let seconds = format!("{seconds:.3}");
    Some(match player {
        "mpv" | "io.mpv.Mpv" => vec![format!("--start={seconds}")],
        "vlc" | "cvlc" | "qvlc" | "nvlc" | "org.videolan.VLC" => {
            vec![format!("--start-time={seconds}")]
        }
        "celluloid" | "io.github.celluloid_player.Celluloid" => {
            vec![format!("--mpv-start={seconds}")]
        }
        "mplayer" => vec!["-ss".to_owned(), seconds],
        _ => return None,
    })
}

pub(super) fn command_line_at(exec: &str, seconds: f64) -> Option<String> {
    let tokens: Vec<String> = glib::shell_parse_argv(exec)
        .ok()?
        .into_iter()
        .map(|token| token.to_string_lossy().into_owned())
        .collect();
    let arguments = start_arguments(&player_name(&tokens)?, seconds)?;
    let insert_at = tokens
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, token)| is_placeholder(token) || *token == "--")
        .map_or(tokens.len(), |(index, _)| index);
    let mut tokens = tokens;
    tokens.splice(insert_at..insert_at, arguments);
    Some(
        tokens
            .iter()
            .map(|token| {
                let plain = token
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-./=:@%+,".contains(&byte));
                if plain && !token.is_empty() || is_placeholder(token) {
                    token.clone()
                } else {
                    glib::shell_quote(token).to_string_lossy().into_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn launch_with_recent_registration(
    app: &gio::AppInfo,
    launcher: &gio::AppInfo,
    files: &[gio::File],
    content_types: &[String],
    context: Option<&impl IsA<gio::AppLaunchContext>>,
    register_recent: impl Fn(&gio::File) -> bool + 'static,
) -> Result<(), glib::Error> {
    // GIO drops files without a local path when expanding %f/%F.
    if !launcher.supports_uris() && requires_uri_handlers(files) {
        return Err(glib::Error::new(
            gio::IOErrorEnum::NotSupported,
            &crate::i18n::tr("This application cannot open files at this location"),
        ));
    }
    launcher.launch(files, context)?;
    if let Some(app_id) = app.id() {
        let known_ids = gio::AppInfo::all()
            .into_iter()
            .filter_map(|candidate| candidate.id().map(|id| id.to_string()))
            .collect::<HashSet<_>>();
        recent_apps::record(content_types, app_id.as_str(), known_ids);
    }
    let candidates: Vec<gio::File> = files
        .iter()
        .filter(|file| !file.has_uri_scheme("recent"))
        .cloned()
        .collect();
    if candidates.is_empty() {
        return Ok(());
    }
    // Advisory history updates must not block launching on slow mounts.
    glib::MainContext::default().spawn_local(async move {
        for file in candidates {
            let file_type = file
                .query_info_future(
                    "standard::type",
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await
                .map(|info| info.file_type());
            // A successful launch remains worth recording when metadata is unavailable.
            if !matches!(
                file_type,
                Ok(gio::FileType::Directory | gio::FileType::Mountable)
            ) {
                register_recent(&file);
            }
        }
    });
    Ok(())
}

fn register_recent_file(file: &gio::File) -> bool {
    let manager = gtk::RecentManager::default();
    let uri = file.uri();
    let added = manager.add_item(uri.as_str());
    if !added {
        tracing::debug!(uri = %uri, "unable to record file in Recent history");
    }
    added
}

fn app_row(app: &gio::AppInfo, default: bool) -> gtk::ListBoxRow {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    line.add_css_class("search-result");
    let icon = gtk::Image::new();
    icon.set_pixel_size(24);
    match app.icon() {
        Some(gicon) => icon.set_from_gicon(&gicon),
        None => icon.set_icon_name(Some("application-x-executable")),
    }
    line.append(&icon);
    let name = nyx_ui::widgets::label(&app.display_name(), "search-name");
    name.set_hexpand(true);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    line.append(&name);
    if default {
        line.append(&nyx_ui::widgets::tag(&crate::i18n::tr("Default"), "accent"));
    }
    let row = gtk::ListBoxRow::new();
    super::accessibility::set_label(&row, &app.display_name());
    if let Some(description) = app.description() {
        super::accessibility::set_description(&row, Some(&description));
    }
    row.set_child(Some(&line));
    row
}

/// Reports `message` over `overlay`, or in a dialog when there is none.
fn report(parent: &gtk::Window, overlay: Option<&gtk::Overlay>, message: &str) {
    match overlay {
        Some(overlay) => nyx_ui::toast::show(overlay, message),
        None => super::modal::show_error_dialog(parent, message, ""),
    }
}

/// The Open With dialog: the apps every file can open with, recent ones first,
/// filtered as you type. `fallback` is the dialog a file with no default app
/// opens, which offers to make the choice the default.
pub(crate) fn dialog(
    parent: &gtk::Window,
    overlay: Option<gtk::Overlay>,
    files: Vec<gio::File>,
    apps: Applications,
    fallback: bool,
) {
    if let Some(reason) = apps.unavailable_reason() {
        let reason = if fallback && apps.content_types.len() <= 1 {
            "No application is registered for this file"
        } else {
            reason
        };
        report(parent, overlay.as_ref(), &crate::i18n::tr(reason));
        return;
    }
    let (dialog, card) = nyx_ui::widgets::dialog(&crate::i18n::tr("Open With"), 460);
    dialog.set_transient_for(Some(parent));
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some(&crate::i18n::tr("Search applications")));
    card.append(&filter);
    let list = gtk::ListBox::new();
    list.add_css_class("search-results");
    super::accessibility::set_label(&list, &crate::i18n::tr("Applications"));
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(320)
        .child(&list)
        .build();
    card.append(&scroller);
    let always = gtk::CheckButton::with_label(&crate::i18n::tr("Always use for this file type"));
    always.set_visible(apps.content_types.len() == 1);
    always.set_active(fallback);
    card.append(&always);
    let mut ordered: Vec<(&'static str, gio::AppInfo)> = Vec::new();
    for (section, group) in [
        ("Recent", &apps.choices.recent),
        ("Recommended", &apps.choices.recommended),
        ("Other applications", &apps.choices.other),
    ] {
        ordered.extend(group.iter().map(|app| (section, app.clone())));
    }
    let shown: Rc<RefCell<Vec<Option<gio::AppInfo>>>> = Rc::default();
    let render = {
        let (list, shown, filter) = (list.clone(), shown.clone(), filter.clone());
        let default = apps.default.clone();
        move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            shown.borrow_mut().clear();
            let query = filter.text().to_lowercase();
            let mut section = "";
            for (group, app) in &ordered {
                if !query.is_empty() && !app.display_name().to_lowercase().contains(&query) {
                    continue;
                }
                if *group != section {
                    section = group;
                    let heading = gtk::ListBoxRow::new();
                    heading.set_selectable(false);
                    heading.set_activatable(false);
                    heading.set_focusable(false);
                    heading.set_child(Some(&nyx_ui::widgets::label(
                        &crate::i18n::tr(group).to_uppercase(),
                        "menu-heading",
                    )));
                    list.append(&heading);
                    shown.borrow_mut().push(None);
                }
                list.append(&app_row(app, default.as_ref().is_some_and(|d| d.equal(app))));
                shown.borrow_mut().push(Some(app.clone()));
            }
        }
    };
    render();
    let r = render.clone();
    filter.connect_search_changed(move |_| r());
    let content_types = apps.content_types.clone();
    let (d, parent_window) = (dialog.clone(), parent.clone());
    list.connect_row_activated(move |_, row| {
        let Some(app) = usize::try_from(row.index())
            .ok()
            .and_then(|i| shown.borrow().get(i).cloned().flatten())
        else {
            return;
        };
        d.close();
        if always.is_active() {
            for content_type in &content_types {
                if let Err(error) = app.set_as_default_for_type(content_type) {
                    tracing::warn!(%content_type, %error, "unable to set default application");
                }
            }
        }
        let context = gtk::prelude::WidgetExt::display(&parent_window).app_launch_context();
        crate::launch_environment::restore_in(&context);
        if let Err(error) = launch(&app, &files, &content_types, Some(&context)) {
            report(&parent_window, overlay.as_ref(), error.message());
        }
    });
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    buttons.append(&cancel);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    dialog.present();
    filter.grab_focus();
}

#[cfg(test)]
mod tests;
