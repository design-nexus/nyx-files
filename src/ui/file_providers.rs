// SPDX-License-Identifier: MIT
use super::thumbnail::ThumbnailSlot;
use crate::assets::icons;
use crate::services::file_providers::{
    self as protocol, Client, Decoration, Feedback, MenuAction, OutcomeStatus, Placement,
    Registration, Request, Update,
};
use crate::services::file_providers::Category;
use gtk::{gdk, gio, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

mod freshness;
mod menus;

use freshness::Freshness;

struct Answer<T> {
    at: Instant,
    generation: u64,
    value: T,
}

const TTL: Duration = Duration::from_secs(5);
// Refresh deadlines do not erase presentation; failure still has a hard bound.
const MAX_AGE: Duration = Duration::from_secs(15);
type MenuKey = (Vec<String>, bool);
type MenuResult = Vec<(usize, MenuAction)>;
struct Provider {
    client: Client,
    icons: HashMap<String, gdk::Texture>,
    id: String,
    name: String,
    heading_icon: Option<gdk::Texture>,
    category: Option<Category>,
    cache: HashMap<String, Answer<Decoration>>,
    menus: HashMap<MenuKey, Answer<Vec<MenuAction>>>,
    freshness: Freshness,
    disconnected: bool,
    last_busy: Option<Instant>,
}
enum Pending {
    Query(usize, u64, Vec<String>),
    Menu(usize, u64, MenuKey),
    Activate(usize, glib::WeakRef<gtk::Widget>),
}
struct Slot {
    widget: glib::WeakRef<ThumbnailSlot>,
    path: String,
}
struct Hub {
    discovery: mpsc::Receiver<Vec<Registration>>,
    discovered: bool,
    providers: Vec<Provider>,
    slots: HashMap<usize, Slot>,
    pending: HashMap<u64, Pending>,
    serial: u64,
    /// Whether the tick timer is running; it stops once discovery finds nothing.
    ticking: bool,
}
thread_local! { static HUB: RefCell<Option<Hub>> = const { RefCell::new(None) }; }
fn spawn_discovery() -> mpsc::Receiver<Vec<Registration>> {
    let (tx, discovery) = mpsc::channel();
    let root = protocol::config_root();
    std::thread::spawn(move || {
        let _ = tx.send(root.map_or_else(Vec::new, |p| protocol::discover(&p)));
    });
    discovery
}
fn with_hub<T>(f: impl FnOnce(&mut Hub) -> T) -> T {
    HUB.with(|cell| {
        if cell.borrow().is_none() {
            cell.replace(Some(Hub {
                discovery: spawn_discovery(),
                discovered: false,
                providers: Vec::new(),
                slots: HashMap::new(),
                pending: HashMap::new(),
                serial: 0,
                ticking: true,
            }));
            glib::timeout_add_local(Duration::from_millis(100), tick_hub);
        }
        f(cell
            .borrow_mut()
            .as_mut()
            .expect("initialized provider hub"))
    })
}
fn tick_hub() -> glib::ControlFlow {
    let idle = with_hub(|hub| {
        hub.tick();
        if hub.has_no_providers() {
            hub.ticking = false;
        }
        !hub.ticking
    });
    if idle {
        glib::ControlFlow::Break
    } else {
        glib::ControlFlow::Continue
    }
}

/// The themed icon standing for a provider without artwork of its own.
pub(crate) fn category_icon(category: Option<Category>) -> &'static str {
    match category {
        Some(Category::Cloud) => icons::CLOUD,
        Some(Category::Devices) => icons::SMARTPHONE,
        Some(Category::VersionControl) => icons::GIT_BRANCH,
        Some(Category::Other) | None => icons::BLOCKS,
    }
}

pub(super) fn bind(slot: &ThumbnailSlot, path: Option<&Path>) {
    let path = path
        .and_then(Path::to_str)
        .filter(|p| Path::new(p).is_absolute() && p.len() <= 16384)
        .map(str::to_owned);
    if slot.provider_path() != path {
        forget(slot.as_ptr() as usize);
        slot.set_decoration(None, None);
        slot.set_provider_path(path);
    }
    if slot.is_mapped() {
        remap(slot);
    }
}

fn admit(slot: &ThumbnailSlot, path: String) -> bool {
    with_hub(|hub| {
        if hub.has_no_providers() {
            return true;
        }
        let id = slot.as_ptr() as usize;
        if hub.slots.get(&id).is_some_and(|s| s.path == path) {
            return true;
        }
        if hub.slots.len() >= 1024 {
            hub.slots.retain(|_, tracked| {
                if let Some(widget) = tracked.widget.upgrade() {
                    if widget.is_mapped() {
                        return true;
                    }
                    widget.set_decoration(None, None);
                }
                false
            });
        }
        if hub.slots.len() >= 1024 {
            return false;
        }
        hub.slots.insert(
            id,
            Slot {
                widget: slot.downgrade(),
                path,
            },
        );
        true
    })
}

pub(super) fn remap(slot: &ThumbnailSlot) {
    let Some(path) = slot.provider_path() else {
        return;
    };
    if admit(slot, path) || !slot.begin_provider_retry() {
        return;
    }
    let weak = slot.downgrade();
    glib::timeout_add_local(Duration::from_millis(250), move || {
        let Some(slot) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if slot.is_mapped() && slot.provider_path().is_some_and(|path| !admit(&slot, path)) {
            return glib::ControlFlow::Continue;
        }
        slot.finish_provider_retry();
        glib::ControlFlow::Break
    });
}

pub(super) fn unmap(slot: &ThumbnailSlot) {
    forget(slot.as_ptr() as usize);
    slot.set_decoration(None, None);
}

pub(super) fn forget(id: usize) {
    HUB.with(|h| {
        if let Ok(mut state) = h.try_borrow_mut()
            && let Some(hub) = state.as_mut()
        {
            hub.slots.remove(&id);
        }
    });
}
impl Hub {
    fn has_no_providers(&self) -> bool {
        self.discovered && self.providers.is_empty()
    }

    fn send(&mut self, provider: usize, mut r: Request, pending: Pending) {
        self.serial += 1;
        r.id = self.serial;
        if self.providers[provider].client.requests.try_send(r).is_ok() {
            self.pending.insert(self.serial, pending);
        } else {
            let provider = &mut self.providers[provider];
            if provider
                .last_busy
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(10))
            {
                eprintln!(
                    "Files provider {}: host queue full or closed; request not sent",
                    provider.id
                );
                provider.last_busy = Some(Instant::now());
            }
            if let Pending::Activate(_, owner) = pending {
                notify(
                    &owner,
                    &format!(
                        "{}: The file provider is busy or unavailable. This action was not sent.",
                        provider.id
                    ),
                );
            }
        }
    }
    fn tick(&mut self) {
        if let Ok(registrations) = self.discovery.try_recv() {
            self.discovered = true;
            if registrations.is_empty() {
                self.slots.clear();
            }
            for r in registrations {
                let icons: HashMap<String, gdk::Texture> = r
                    .icons
                    .iter()
                    .filter_map(|(k, v)| {
                        gdk::Texture::from_bytes(&glib::Bytes::from(v))
                            .ok()
                            .map(|t| (k.clone(), t))
                    })
                    .collect();
                let id = r.manifest.id.clone();
                let name = r.manifest.name.clone().unwrap_or_else(|| id.clone());
                let heading_icon = r
                    .manifest
                    .icon
                    .as_ref()
                    .and_then(|icon| icons.get(icon))
                    .cloned();
                let category = r.manifest.category.as_deref().and_then(Category::from_id);
                self.providers.push(Provider {
                    client: protocol::start(r),
                    id,
                    name,
                    heading_icon,
                    category,
                    icons,
                    cache: HashMap::new(),
                    menus: HashMap::new(),
                    freshness: Freshness::default(),
                    disconnected: false,
                    last_busy: None,
                });
            }
        }
        for index in 0..self.providers.len() {
            if self.providers[index].disconnected {
                continue;
            }
            let (updates, closed) = self.providers[index].client.updates.drain();
            for update in updates {
                match update {
                    Update::Offline { unsent, in_flight } => {
                        let affected: Vec<_> = unsent.iter().copied().chain(in_flight).collect();
                        self.offline(index, &unsent, Some(&affected));
                    }
                    Update::Reply(reply) if reply.event.is_some() => {
                        self.providers[index]
                            .freshness
                            .invalidate(reply.paths.as_deref(), reply.revision);
                    }
                    Update::Reply(reply) => {
                        let Some(pending) = reply.id.and_then(|id| self.pending.remove(&id)) else {
                            continue;
                        };
                        let provider = &mut self.providers[index];
                        let generation = if reply.revision.is_some() {
                            provider.freshness.generation()
                        } else {
                            match &pending {
                                Pending::Query(_, generation, _)
                                | Pending::Menu(_, generation, _) => *generation,
                                _ => 0,
                            }
                        };
                        match pending {
                            Pending::Query(i, _, paths)
                                if i == index
                                    && (reply.error.is_some()
                                        || provider.freshness.accept(&paths, reply.revision)) =>
                            {
                                let new_count = paths
                                    .iter()
                                    .filter(|path| !provider.cache.contains_key(*path))
                                    .count();
                                let excess =
                                    (provider.cache.len() + new_count).saturating_sub(2048);
                                if excess > 0 {
                                    let mut oldest: Vec<_> = provider
                                        .cache
                                        .iter()
                                        .map(|(path, answer)| (answer.at, path.clone()))
                                        .collect();
                                    oldest.sort_unstable();
                                    for (_, path) in oldest.into_iter().take(excess) {
                                        provider.cache.remove(&path);
                                    }
                                }
                                for path in &paths {
                                    provider.cache.insert(
                                        path.clone(),
                                        Answer {
                                            at: Instant::now(),
                                            generation,
                                            value: Decoration {
                                                path: path.clone(),
                                                badge: None,
                                                description: String::new(),
                                                priority: 0,
                                            },
                                        },
                                    );
                                }
                                for decoration in reply.decorations.unwrap_or_default() {
                                    if paths.contains(&decoration.path) {
                                        provider.cache.insert(
                                            decoration.path.clone(),
                                            Answer {
                                                at: Instant::now(),
                                                generation,
                                                value: decoration,
                                            },
                                        );
                                    }
                                }
                            }
                            Pending::Menu(i, _, key)
                                if i == index
                                    && (reply.error.is_some()
                                        || provider.freshness.accept(&key.0, reply.revision)) =>
                            {
                                if provider.menus.len() >= 32
                                    && !provider.menus.contains_key(&key)
                                    && let Some(oldest) = provider
                                        .menus
                                        .iter()
                                        .min_by_key(|(_, answer)| answer.at)
                                        .map(|(key, _)| key.clone())
                                {
                                    provider.menus.remove(&oldest);
                                }
                                provider.menus.insert(
                                    key,
                                    Answer {
                                        at: Instant::now(),
                                        generation,
                                        value: reply.actions.unwrap_or_default(),
                                    },
                                );
                            }
                            Pending::Activate(i, owner) if i == index => {
                                match reply.feedback() {
                                    Feedback::None => (),
                                    Feedback::Notice => {
                                        notice(&owner, &provider.name, &reply.message);
                                    }
                                    Feedback::Dialog | Feedback::Unrecognized => {
                                        notify(&owner, &activation_summary(&provider.id, *reply));
                                    }
                                }
                                self.invalidate(index);
                            }
                            _ => (),
                        }
                    }
                }
            }
            if closed {
                self.offline(index, &[], None);
                self.providers[index].disconnected = true;
            }
        }
        self.slots.retain(|_, s| s.widget.upgrade().is_some());
        let visible: Vec<_> = self
            .slots
            .values()
            .filter(|s| s.widget.upgrade().is_some_and(|w| w.is_mapped()))
            .map(|s| s.path.clone())
            .collect();
        for index in 0..self.providers.len() {
            let provider = &self.providers[index];
            if provider.disconnected {
                continue;
            }
            let mut missing: Vec<_> = visible.iter().filter(|path| !provider.cache.get(*path).is_some_and(|answer| provider.freshness.current(std::slice::from_ref(*path), answer.generation) && answer.at.elapsed() < TTL)
                && !self.pending.values().any(|p| matches!(p,Pending::Query(i,_,paths) if *i == index && paths.contains(path)))).cloned().collect();
            missing.sort_by(|a, b| {
                provider
                    .cache
                    .get(a)
                    .map(|answer| answer.at)
                    .cmp(&provider.cache.get(b).map(|answer| answer.at))
                    .then(a.cmp(b))
            });
            missing.dedup();
            let missing = protocol::query_batch(missing);
            if !missing.is_empty() {
                let epoch = provider.freshness.generation();
                self.send(
                    index,
                    request("query", missing.clone(), false),
                    Pending::Query(index, epoch, missing),
                );
            }
        }
        for s in self.slots.values() {
            if let Some(widget) = s.widget.upgrade() {
                let mut decorations: Vec<_> = self
                    .providers
                    .iter()
                    .filter_map(|p| {
                        p.cache
                            .get(&s.path)
                            .filter(|answer| answer.at.elapsed() < MAX_AGE)
                            .and_then(|answer| {
                                answer
                                    .value
                                    .badge
                                    .as_ref()
                                    .and_then(|badge| p.icons.get(badge))
                                    .map(|texture| {
                                        (
                                            answer.value.priority,
                                            &p.id,
                                            texture,
                                            answer.value.description.as_str(),
                                        )
                                    })
                            })
                    })
                    .collect();
                decorations.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
                let description = decorations
                    .iter()
                    .map(|(_, id, _, text)| format!("{id}: {text}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                widget.set_decoration(
                    decorations.first().map(|(_, _, texture, _)| *texture),
                    (!decorations.is_empty()).then_some(description.as_str()),
                );
            }
        }
    }
    fn offline(&mut self, index: usize, unsent: &[u64], affected: Option<&[u64]>) {
        self.providers[index].freshness = Freshness::default();
        self.providers[index].cache.clear();
        self.providers[index].menus.clear();
        for pending in self.pending.values_mut() {
            match pending {
                Pending::Query(provider, generation, _)
                | Pending::Menu(provider, generation, _)
                    if *provider == index =>
                {
                    *generation = self.providers[index].freshness.generation();
                }
                _ => (),
            }
        }
        let ids: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(id, pending)| match pending {
                Pending::Query(i, _, _) | Pending::Menu(i, _, _) | Pending::Activate(i, _)
                    if *i == index && affected.is_none_or(|ids| ids.contains(id)) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        for id in ids {
            if let Some(Pending::Activate(_, owner)) = self.pending.remove(&id) {
                let message = if unsent.contains(&id) {
                    "The provider disconnected before this action was sent."
                } else {
                    "The provider disconnected. The action may already have been accepted; check its state before retrying."
                };
                notify(&owner, &format!("{}: {message}", self.providers[index].id));
            }
        }
    }
    fn invalidate(&mut self, index: usize) {
        self.providers[index].freshness.invalidate(None, None);
    }
    /// Whether a provider has yet to answer the menu for `key`.
    fn menu_pending(&self, key: &MenuKey) -> bool {
        self.pending
            .values()
            .any(|r| matches!(r, Pending::Menu(_, _, k) if k == key))
            || !self.discovered
    }
    fn menu(&mut self, key: &MenuKey) -> MenuResult {
        let mut out = Vec::new();
        for i in 0..self.providers.len() {
            let p = &self.providers[i];
            if p.disconnected {
                continue;
            }
            if let Some(answer) = p
                .menus
                .get(key)
                .filter(|answer| answer.at.elapsed() < MAX_AGE)
            {
                out.extend(answer.value.iter().cloned().map(|a| (i, a)));
            }
            if !p.menus.get(key).is_some_and(|answer| {
                p.freshness.current(&key.0, answer.generation) && answer.at.elapsed() < TTL
            }) && !self
                .pending
                .values()
                .any(|r| matches!(r, Pending::Menu(j,_,k) if *j == i && k == key))
            {
                let epoch = p.freshness.generation();
                self.send(
                    i,
                    request("menu", key.0.clone(), key.1),
                    Pending::Menu(i, epoch, key.clone()),
                );
            }
        }
        out
    }
}
fn request(method: &str, paths: Vec<String>, background: bool) -> Request {
    Request {
        version: 1,
        id: 0,
        method: method.into(),
        paths,
        background,
        action: None,
        context: None,
    }
}
fn activation_summary(provider: &str, reply: protocol::Reply) -> String {
    let message = if let Some(outcome) = &reply.outcome {
        let mut status = match outcome.status {
            OutcomeStatus::Accepted => "Accepted",
            OutcomeStatus::Rejected => "Rejected",
            OutcomeStatus::Partial => "Partially accepted",
            OutcomeStatus::Unknown => "Outcome unknown",
        }
        .to_owned();
        if let Some((accepted, total)) = outcome.accepted.zip(outcome.total) {
            if outcome.status == OutcomeStatus::Unknown {
                status.push_str(&format!(" (at least {accepted}/{total} accepted)"));
            } else {
                status.push_str(&format!(" ({accepted}/{total})"));
            }
        }
        if let Some(job) = &outcome.job {
            status.push_str(&format!("\nJob: {job}"));
        }
        format!("{status}\n\n{}", reply.message)
    } else {
        reply.message
    };
    format!(
        "{provider}: {}",
        if message.is_empty() {
            "The provider did not report an outcome."
        } else {
            &message
        }
    )
}

/// Falls back to the dialog when the menu owner has no overlay to host a notice.
fn notice(owner: &glib::WeakRef<gtk::Widget>, heading: &str, message: &str) {
    let Some(widget) = owner.upgrade() else {
        return;
    };
    let overlay = widget.clone().downcast::<gtk::Overlay>().ok().or_else(|| {
        widget
            .ancestor(gtk::Overlay::static_type())?
            .downcast()
            .ok()
    });
    match overlay {
        Some(overlay) => {
            super::notice::show_notice(&overlay, Some(heading), message)
        }
        None => notify(owner, &format!("{heading}: {message}")),
    }
}

fn notify(owner: &glib::WeakRef<gtk::Widget>, message: &str) {
    let Some(owner) = owner.upgrade() else {
        return;
    };
    super::modal::show_information_dialog(&owner, &crate::i18n::tr("File availability"), message);
}
/// The caller owns the epoch: rebuilding/closing a menu cancels its old watcher.
/// Asks the providers what they offer for `paths` and calls `ready` with it as
/// menu entries once every provider has answered (or after a few seconds);
/// the entries are empty when none offers anything.
pub(crate) fn collect_menu(
    owner: &gtk::Widget,
    paths: Vec<PathBuf>,
    background: bool,
    ready: impl FnOnce(Vec<nyx_ui::menu::Entry>) + 'static,
) {
    const WAIT: Duration = Duration::from_secs(3);
    let paths = paths
        .iter()
        .map(|p| p.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>();
    let Some(paths) = paths.filter(|paths| {
        !paths.is_empty() && paths.len() <= protocol::PATH_LIMIT && protocol::selection_fits(paths)
    }) else {
        ready(Vec::new());
        return;
    };
    let key = (paths, background);
    let model = gio::Menu::new();
    let group = gio::SimpleActionGroup::new();
    let mut renderer = menus::Renderer::new(
        (model.clone(), None),
        group.clone(),
        owner.downgrade(),
        glib::WeakRef::new(),
        key.clone(),
        std::rc::Rc::new(std::cell::Cell::new(0)),
        0,
    );
    let started = Instant::now();
    let mut ready = Some(ready);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        let result = with_hub(|h| h.menu(&key));
        renderer.update(result);
        if with_hub(|h| h.menu_pending(&key)) && started.elapsed() < WAIT {
            return glib::ControlFlow::Continue;
        }
        if let Some(ready) = ready.take() {
            ready(menu_entries(model.upcast_ref(), &group));
        }
        glib::ControlFlow::Break
    });
}

/// The rendered provider menu as `nyx_ui::menu` entries: sections become headed
/// groups, submenus stay submenus, items activate the provider's action.
fn menu_entries(model: &gio::MenuModel, group: &gio::SimpleActionGroup) -> Vec<nyx_ui::menu::Entry> {
    use nyx_ui::menu;
    let string = |index: i32, name: &str| {
        model
            .item_attribute_value(index, name, Some(glib::VariantTy::STRING))
            .and_then(|value| value.get::<String>())
            .map(|value| value.replace("__", "_"))
    };
    let mut entries = Vec::new();
    for index in 0..model.n_items() {
        let label = string(index, "label");
        if let Some(section) = model.item_link(index, "section") {
            let items = menu_entries(&section, group);
            if items.is_empty() {
                continue;
            }
            if !entries.is_empty() {
                entries.push(menu::separator());
            }
            if let Some(label) = label {
                entries.push(menu::heading(&label));
            }
            entries.extend(items);
        } else if let Some(submenu) = model.item_link(index, "submenu") {
            entries.push(menu::sub(&label.unwrap_or_default(), menu_entries(&submenu, group)));
        } else if let (Some(label), Some(action)) = (label, string(index, "action")) {
            let name = action.trim_start_matches("provider.").to_owned();
            let group = group.clone();
            entries.push(menu::item(&label, move || group.activate_action(&name, None)));
        }
    }
    entries
}

#[cfg(test)]
mod tests;
