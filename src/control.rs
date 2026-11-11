//! Files on the control interface (`nyx_core::control`, API 2), for the `nyx` command,
//! Glimpse's Files panel and Nyx Assistant: list and search folders in the home folder,
//! open and reveal, and copy, move and trash through the same operations as the
//! window (Keep both on a name clash, Trash rather than delete).

use crate::adapters::RevealRequest;
use crate::model::{EntryKind, FileEntry, Location, MetadataValue};
use crate::services::{
    DeleteRequest, OperationEvent, OperationProvider, OperationRequestId, PasteItem, PasteRequest, TransferConflict,
};
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use nyx_actions::{Error, ErrorKind};
use nyx_core::control::Answer;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Listings and searches stop here.
const MAX_ITEMS: usize = 500;
const MAX_HITS: usize = 50;
const SEARCH_BUDGET: Duration = Duration::from_secs(2);

/// Where a call may look: these, or a path in the home folder.
const PLACES: &[(&str, glib::UserDirectory, &str)] = &[
    ("desktop", glib::UserDirectory::Desktop, "Desktop"),
    ("documents", glib::UserDirectory::Documents, "Documents"),
    ("downloads", glib::UserDirectory::Downloads, "Downloads"),
    ("music", glib::UserDirectory::Music, "Music"),
    ("pictures", glib::UserDirectory::Pictures, "Pictures"),
    ("videos", glib::UserDirectory::Videos, "Videos"),
];

/// A place's folder: the user's setting (user-dirs.dirs), else its usual name in home.
fn place(dir: glib::UserDirectory, usual: &str) -> Option<PathBuf> {
    glib::user_special_dir(dir).filter(|p| p.is_dir()).or_else(|| Some(home().join(usual)).filter(|p| p.is_dir()))
}

pub fn description(app_id: &str) -> Value {
    let path = json!({"type": "string", "format": "path", "description": "a folder or file in the home folder (~/…), or home, desktop, documents, downloads, music, pictures or videos"});
    let paths = json!({"type": "array", "items": {"type": "string", "format": "path"}, "minItems": 1});
    json!({
        "tools": [
            {"name": "files.list", "description": "What a folder in the home folder holds: folders first, then files, with sizes and dates.",
             "inputSchema": {"type": "object", "properties": {"path": path, "hidden": {"type": "boolean"}}, "additionalProperties": false},
             "annotations": {"title": "List a folder"}, "x-nyx": {"icon": "folder-symbolic", "risk": "read_only", "positional": ["path"], "aliases": ["ls", "browse"], "result": "list"}},
            {"name": "files.locations", "description": "The usual places: home, desktop, documents, downloads, music, pictures and videos.",
             "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
             "annotations": {"title": "Places"}, "x-nyx": {"icon": "user-home-symbolic", "risk": "read_only", "aliases": ["places"], "result": "list"}},
            {"name": "files.search", "description": "Find files and folders by name under a folder in the home folder (the home folder by default).",
             "inputSchema": {"type": "object", "properties": {"query": {"type": "string", "minLength": 1}, "path": path}, "required": ["query"], "additionalProperties": false},
             "annotations": {"title": "Search files"}, "x-nyx": {"risk": "read_only", "positional": ["query"], "aliases": ["find"], "result": "list"}},
            {"name": "files.open", "description": "Open a file with its usual app, or a folder in Files.",
             "inputSchema": {"type": "object", "properties": {"path": path}, "required": ["path"], "additionalProperties": false},
             "annotations": {"title": "Open"}, "x-nyx": {"risk": "writes", "confirmation": "never", "positional": ["path"], "result": "none"}},
            {"name": "files.reveal", "description": "Show a folder, or the folder a file is in with the file selected, in Files.",
             "inputSchema": {"type": "object", "properties": {"path": path}, "additionalProperties": false},
             "annotations": {"title": "Show in Files"}, "x-nyx": {"risk": "writes", "confirmation": "never", "positional": ["path"], "aliases": ["show"], "result": "none"}},
            {"name": "files.copy", "description": "Copy files or folders into a folder (a name already there keeps both).",
             "inputSchema": {"type": "object", "properties": {"sources": paths, "destination": path}, "required": ["sources", "destination"], "additionalProperties": false},
             "annotations": {"title": "Copy"}, "x-nyx": {"risk": "writes", "positional": ["sources", "destination"], "aliases": ["cp"], "timeout_ms": 300000, "result": "message"}},
            {"name": "files.move", "description": "Move files or folders into a folder (a name already there keeps both).",
             "inputSchema": {"type": "object", "properties": {"sources": paths, "destination": path}, "required": ["sources", "destination"], "additionalProperties": false},
             "annotations": {"title": "Move"}, "x-nyx": {"risk": "writes", "positional": ["sources", "destination"], "aliases": ["mv"], "timeout_ms": 300000, "result": "message"}},
            {"name": "files.trash", "description": "Move files or folders to the Trash.",
             "inputSchema": {"type": "object", "properties": {"paths": paths}, "required": ["paths"], "additionalProperties": false},
             "annotations": {"title": "Move to Trash"}, "x-nyx": {"risk": "destructive", "positional": ["paths"], "aliases": ["rm"], "timeout_ms": 120000, "result": "message"}}
        ],
        "nyx": {
            "api": 2, "app": app_id, "name": "Files", "icon": crate::APPLICATION_ID, "namespaces": ["files"], "aliases": ["folders"],
            "bus_name": app_id, "activation": "dbus", "default": "files.list", "open": {"action": "files.reveal"},
            "topics": [{"name": "changes", "sensitive": true}],
            "panel": {"id": "files", "topics": ["changes"], "size": {"min": [480, 360], "preferred": [560, 420]}}
        }
    })
}

/// Register the interface (from `connect_startup`). Started headless for a call, Files
/// stays up a while after the last one, so a panel's next step needn't start it again.
pub fn serve(application: &gtk::Application) {
    let id = application.application_id().map(|s| s.to_string()).unwrap_or_else(|| crate::APPLICATION_ID.to_string());
    if application.flags().contains(gio::ApplicationFlags::IS_SERVICE) && application.inactivity_timeout() == 0 {
        application.set_inactivity_timeout(30_000);
    }
    let app = application.clone();
    let Some(service) = nyx_core::control::serve_with(application, description(&id), move |request, answer| handle(&app, &request.action, &request.arguments, answer)) else {
        return;
    };
    service.on_watch(|_, on| {
        if on {
            publish_changes();
        }
    });
    SERVICE.with(|s| *s.borrow_mut() = Some(service));
}

thread_local! {
    static SERVICE: RefCell<Option<nyx_core::control::Service>> = const { RefCell::new(None) };
    /// The folders listed last (at most eight), watched, and how often each changed:
    /// the `changes` topic, `{changed: {path: count}}`, which a panel reloads by.
    static WATCHED: RefCell<Vec<(PathBuf, gio::FileMonitor)>> = const { RefCell::new(Vec::new()) };
    static CHANGED: RefCell<std::collections::BTreeMap<String, u64>> = const { RefCell::new(std::collections::BTreeMap::new()) };
    static PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn follow(dir: &Path) {
    WATCHED.with(|w| {
        let mut w = w.borrow_mut();
        if let Some(i) = w.iter().position(|(p, _)| p == dir) {
            let again = w.remove(i);
            w.push(again);
            return;
        }
        let Ok(monitor) = gio::File::for_path(dir).monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else { return };
        let key = dir.display().to_string();
        monitor.connect_changed(move |_, _, _, _| changed(&key));
        w.push((dir.to_path_buf(), monitor));
        if w.len() > 8 {
            let (_, old) = w.remove(0);
            old.cancel();
        }
    });
}

/// A burst of changes (a copy of many files) is one update, 300 ms later.
fn changed(key: &str) {
    CHANGED.with(|c| *c.borrow_mut().entry(key.to_string()).or_default() += 1);
    if !PENDING.with(|p| p.replace(true)) {
        glib::timeout_add_local_once(Duration::from_millis(300), || {
            PENDING.with(|p| p.set(false));
            publish_changes();
        });
    }
}

fn publish_changes() {
    let Some(service) = SERVICE.with(|s| s.borrow().clone()) else { return };
    if service.watched("changes") {
        service.publish("changes", json!({ "changed": CHANGED.with(|c| c.borrow().clone()) }));
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidArguments, message)
}

fn home() -> PathBuf {
    glib::home_dir()
}

/// A place's name, `~`, or a path inside the home folder (the home folder itself too).
fn resolve(input: &str) -> Result<PathBuf, Error> {
    let word = input.trim();
    if word.is_empty() || word == "~" || word.eq_ignore_ascii_case("home") {
        return Ok(home());
    }
    if let Some((_, dir, usual)) = PLACES.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(word)) {
        return place(*dir, usual).ok_or_else(|| invalid(format!("there's no {word} folder")));
    }
    nyx_core::control::home_path(word).map_err(invalid)
}

fn existing(input: &str) -> Result<PathBuf, Error> {
    let path = resolve(input)?;
    if !path.exists() {
        return Err(invalid(format!("{} doesn't exist", path.display())));
    }
    Ok(path)
}

fn shown(path: &Path) -> String {
    nyx_core::paths::pretty(path)
}

fn item(path: &Path, meta: Option<&std::fs::Metadata>) -> Value {
    let folder = meta.is_some_and(|m| m.is_dir());
    let size = meta.filter(|m| m.is_file()).map(|m| m.len());
    let modified = meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| shown(path));
    let content_type = if folder { "inode/directory".to_string() } else { gio::content_type_guess(Some(path), None).0.to_string() };
    let subtitle = match size {
        Some(bytes) => glib::format_size(bytes).to_string(),
        None if folder => "Folder".into(),
        None => String::new(),
    };
    json!({"title": name, "subtitle": subtitle, "path": path.display().to_string(), "kind": if folder { "folder" } else { "file" },
           "size": size, "modified": modified, "type": content_type, "icon": gio::content_type_get_generic_icon_name(&content_type).map(|i| format!("{i}-symbolic"))})
}

fn list(args: &Value) -> Result<Value, Error> {
    let dir = existing(args["path"].as_str().unwrap_or("~"))?;
    if !dir.is_dir() {
        return Err(invalid(format!("{} isn't a folder", shown(&dir))));
    }
    let hidden = args["hidden"] == true;
    let mut entries: Vec<(bool, String, PathBuf, std::fs::Metadata)> = std::fs::read_dir(&dir)
        .map_err(|e| Error::new(ErrorKind::Failed, format!("{} can't be read: {e}", shown(&dir))))?
        .flatten()
        .filter(|e| hidden || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let meta = std::fs::metadata(e.path()).ok()?;
            Some((!meta.is_dir(), e.file_name().to_string_lossy().to_lowercase(), e.path(), meta))
        })
        .collect();
    entries.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let total = entries.len();
    let items: Vec<Value> = entries.iter().take(MAX_ITEMS).map(|(_, _, p, m)| item(p, Some(m))).collect();
    let parent = dir.parent().filter(|p| p.starts_with(home()) && dir != home()).map(|p| p.display().to_string());
    Ok(json!({"items": items, "path": dir.display().to_string(), "shown": shown(&dir), "parent": parent, "total": total}))
}

fn locations() -> Value {
    let mut items = vec![item(&home(), std::fs::metadata(home()).ok().as_ref())];
    items[0]["title"] = json!("Home");
    for (name, dir, usual) in PLACES {
        if let Some(path) = place(*dir, usual).filter(|p| *p != home()) {
            let mut v = item(&path, std::fs::metadata(&path).ok().as_ref());
            v["place"] = json!(name);
            items.push(v);
        }
    }
    json!({"items": items})
}

fn search(args: &Value) -> Result<Value, Error> {
    let query = args["query"].as_str().unwrap_or("").trim().to_lowercase();
    if query.is_empty() {
        return Err(invalid("say what to look for"));
    }
    let root = existing(args["path"].as_str().unwrap_or("~"))?;
    let started = std::time::Instant::now();
    let mut hits = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root.clone(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if hits.len() >= MAX_HITS || started.elapsed() > SEARCH_BUDGET {
            break;
        }
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if name.to_lowercase().contains(&query) {
                hits.push(item(&e.path(), Some(&meta)));
            }
            if meta.is_dir() && depth < 6 {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
    let finished = queue.is_empty() || hits.len() < MAX_HITS && started.elapsed() <= SEARCH_BUDGET;
    Ok(json!({"items": hits, "path": root.display().to_string(), "complete": finished}))
}

fn entry(path: &Path) -> FileEntry {
    let folder = path.is_dir();
    FileEntry {
        location: Location::local(path),
        thumbnail_path: None,
        native_name: path.file_name().unwrap_or_default().to_owned(),
        display_name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        kind: if folder { EntryKind::Directory } else { EntryKind::File },
        size: MetadataValue::Unknown,
        modified_unix_seconds: MetadataValue::Unknown,
        recent_unix_seconds: MetadataValue::Unknown,
        is_hidden: false,
        mode: MetadataValue::Unknown,
        image_dimensions: MetadataValue::Unknown,
        child_count: MetadataValue::Unknown,
        duration_seconds: MetadataValue::Unknown,
        recent_uri: None,
    }
}

fn request_id() -> OperationRequestId {
    // Apart from the window's own counter.
    static NEXT: AtomicU64 = AtomicU64::new(1 << 48);
    OperationRequestId(NEXT.fetch_add(1, Ordering::Relaxed))
}

fn paths_of(args: &Value, key: &str) -> Result<Vec<PathBuf>, Error> {
    args[key].as_array().into_iter().flatten().map(|v| existing(v.as_str().unwrap_or(""))).collect()
}

/// Copy or move through the window's operations; answers once they report the end.
fn transfer(args: &Value, move_sources: bool, answer: Answer) {
    let (sources, destination) = match (paths_of(args, "sources"), existing(args["destination"].as_str().unwrap_or(""))) {
        (Ok(s), Ok(d)) if d.is_dir() => (s, d),
        (Ok(_), Ok(d)) => return answer(Err(invalid(format!("{} isn't a folder", shown(&d))))),
        (Err(e), _) | (_, Err(e)) => return answer(Err(e)),
    };
    let n = sources.len();
    // Keep both only where the name is taken; otherwise the item keeps its own name.
    let items = sources
        .into_iter()
        .map(|s| {
            let taken = s.file_name().is_some_and(|n| destination.join(n).exists());
            let conflict = if taken { TransferConflict::KeepBoth } else { TransferConflict::FailIfExists };
            PasteItem { source: Location::local(s), conflict, target_name: None }
        })
        .collect();
    let request = PasteRequest { id: request_id(), destination: Location::local(&destination), items, move_sources };
    let verb = if move_sources { "Moved" } else { "Copied" };
    let target = shown(&destination);
    run(answer, move |emit| crate::adapters::LocalOperationProvider.paste(request, emit), move |event| match event {
        OperationEvent::Pasted { .. } => Some(Ok(json!({"message": format!("{verb} {} to {target}", count(n))}))),
        OperationEvent::TransferFailed { message, .. } => Some(Err(Error::new(ErrorKind::Failed, message.clone()))),
        _ => None,
    });
}

fn trash(args: &Value, answer: Answer) {
    let paths = match paths_of(args, "paths") {
        Ok(p) => p,
        Err(e) => return answer(Err(e)),
    };
    let n = paths.len();
    let request = DeleteRequest { id: request_id(), entries: paths.iter().map(|p| entry(p)).collect(), permanent: false };
    run(answer, move |emit| crate::adapters::LocalOperationProvider.delete(request, emit), move |event| match event {
        OperationEvent::Deleted { .. } => Some(Ok(json!({"message": format!("Moved {} to the Trash", count(n))}))),
        OperationEvent::CompletedWithErrors { message, .. } => Some(Err(Error::new(ErrorKind::Failed, message.clone()))),
        _ => None,
    });
}

fn count(n: usize) -> String {
    if n == 1 { "1 item".into() } else { format!("{n} items") }
}

/// Start an operation and answer from the first event `end` turns into a result.
fn run(
    answer: Answer,
    start: impl FnOnce(Rc<dyn Fn(OperationEvent)>) -> crate::services::LoadHandle,
    end: impl Fn(&OperationEvent) -> Option<Result<Value, Error>> + 'static,
) {
    let answer: Rc<RefCell<Option<Answer>>> = Rc::new(RefCell::new(Some(answer)));
    let handle: Rc<RefCell<Option<crate::services::LoadHandle>>> = Rc::default();
    let (a, h) = (answer.clone(), handle.clone());
    let emit: Rc<dyn Fn(OperationEvent)> = Rc::new(move |event| {
        if let Some(result) = end(&event)
            && let Some(reply) = a.borrow_mut().take()
        {
            reply(result);
            // The operation is over; its handle can go.
            let h = h.clone();
            glib::idle_add_local_once(move || drop(h.borrow_mut().take()));
        }
    });
    *handle.borrow_mut() = Some(start(emit));
}

fn handle(app: &gtk::Application, action: &str, args: &Value, answer: Answer) {
    match action {
        "files.list" => {
            let listed = list(args);
            if let Ok(v) = &listed
                && let Some(dir) = v["path"].as_str()
            {
                follow(Path::new(dir));
            }
            answer(listed)
        }
        "files.locations" => answer(Ok(locations())),
        "files.search" => answer(search(args)),
        "files.open" => answer((|| {
            let path = existing(args["path"].as_str().unwrap_or(""))?;
            if path.is_dir() {
                crate::interface::present_reveal(app, RevealRequest { directory: Location::local(&path), selection: Vec::new(), properties: false });
                return Ok(Value::Null);
            }
            let uri = gio::File::for_path(&path).uri();
            gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>).map_err(|e| Error::new(ErrorKind::Failed, e.to_string()))?;
            Ok(Value::Null)
        })()),
        "files.reveal" => answer((|| {
            let path = existing(args["path"].as_str().unwrap_or("~"))?;
            let request = if path.is_dir() {
                RevealRequest { directory: Location::local(&path), selection: Vec::new(), properties: false }
            } else {
                let folder = path.parent().map(Path::to_path_buf).unwrap_or_else(home);
                RevealRequest { directory: Location::local(folder), selection: vec![Location::local(&path)], properties: false }
            };
            crate::interface::present_reveal(app, request);
            Ok(Value::Null)
        })()),
        "files.copy" => transfer(args, false, answer),
        "files.move" => transfer(args, true, answer),
        "files.trash" => trash(args, answer),
        other => answer(Err(Error::new(ErrorKind::UnknownAction, format!("{other} isn't an action here")))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_description_is_api_2() {
        let d = nyx_actions::Description::from_value(&description(crate::APPLICATION_ID), None).unwrap();
        assert_eq!(d.app.default.as_deref(), Some("files.list"));
        assert!(d.action("files.trash").unwrap().asks());
        assert!(!d.action("files.copy").unwrap().asks());
        assert_eq!(d.app.panel.as_ref().map(|p| p.id.as_str()), Some("files"));
    }

    #[test]
    fn control_resolves_places_and_home_only() {
        assert_eq!(resolve("home").unwrap(), home());
        assert_eq!(resolve("~").unwrap(), home());
        assert!(resolve("/etc/passwd").is_err());
        assert!(resolve("~/../..").is_err());
    }

    #[test]
    fn control_lists_folders_first() {
        let dir = tempfile::Builder::new().prefix("files-control-").tempdir_in(home()).unwrap();
        std::fs::create_dir(dir.path().join("b-folder")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        std::fs::write(dir.path().join(".hidden"), "x").unwrap();
        let listed = list(&json!({"path": dir.path().display().to_string()})).unwrap();
        let names: Vec<&str> = listed["items"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["b-folder", "a.txt"]);
        let found = search(&json!({"query": "A.T", "path": dir.path().display().to_string()})).unwrap();
        assert_eq!(found["items"][0]["title"], "a.txt");
    }
}
