# Architecture Principles

This document records boundaries and constraints, not a frozen class hierarchy. Abstractions should be introduced where behavior varies, work crosses an asynchronous boundary, or a subsystem needs isolated tests.

## Principles

1. **Model product concepts, not widgets.** Navigation paths, locations, entries, selections, operations, and previews must not depend on a specific view instance.
2. **Keep the UI declarative.** Widgets render state and emit intent; they do not perform filesystem work directly.
3. **Make stale work harmless.** Navigation, peek, search, metadata, and preview requests carry cancellation or generation identity.
4. **Stream bounded results.** Large directories and searches arrive in batches with backpressure.
5. **Use capability boundaries.** Search, preview, themes, settings, and file operations expose the capabilities the application needs rather than leaking backend APIs.
6. **Prefer concrete code until variation is real.** Do not create a trait for every type. Extract a boundary when there is a second implementation, a test substitute, or a meaningful isolation requirement.
7. **Keep extensions outside the trusted core.** A future public extension system should use a versioned out-of-process or sandboxed protocol rather than Rust's unstable dynamic-library ABI.
8. **Preserve native paths.** Internal paths must not assume valid UTF-8.
9. **Make observability part of the design.** Slow requests, cancellation, operation failures, and provider errors should be traceable.

## Proposed layers

```text
UI
  Renders application state and sends user intents
        │
Application
  Navigation, selection, history, commands, orchestration
        │
Capabilities
  Files, operations, search, previews, themes, settings
        │
Adapters
  Local filesystem, desktop integration, tools, theme sources
```

Dependencies point inward. A filesystem adapter must not manipulate widgets, and a preview provider must not own navigation state.

## Core product models

- `Location`: a browsable destination with a stable identity
- `FileEntry`: native name/path, type, metadata availability, and capabilities
- `NavigationPath`: committed locations represented by Miller columns
- `PeekState`: temporary location, origin, request generation, and lifecycle
- `SelectionState`: active column, focused item, and multi-selection
- `ViewPreferences`: mode, density, type grouping, sorting, hidden files, and thumbnail policy
- `Operation`: queued file mutation with progress and final outcome
- `PreviewRequest` / `Preview`: bounded request and renderable result
- `SearchQuery` / `SearchResult`: explicit scope and streaming result
- `Theme`: validated semantic tokens with fallbacks

Models should distinguish “unknown/not loaded” from meaningful empty values.

### Interface

`src/interface` is Files' window and file chooser, built on nyx-ui and styled by nyx-theme
(`theme.rs` installs the suite stylesheet plus `style.css` and `parts.css`). Each tab owns one
`app::Browser`; the views are observers of its `BrowserEvent` stream and send intents back to it.
No widget performs filesystem work, and no view keeps navigation or selection state the browser
does not also have.

| Responsibility | Module |
| --- | --- |
| Window: header, tab row, sidebar, pages, key routing, presence | `window.rs` |
| Starting windows: restored tabs, file arguments, FileManager1 reveals, `--unlock-volume` | `launch.rs` |
| A tab: its browser, views, preview drawer, shortcut footer, filter results, freshness, visits | `tab.rs` |
| List models over browser columns and the row hooks (bind, menu, drag, drop, hover, click) | `model.rs` |
| List (`ColumnView`), Icons (`GridView`), Columns (one `ListView` per column) | `list.rs`, `icons.rs`, `columns.rs` |
| Sidebar: places, pins, devices, password drives, cloud; drops and pin reordering | `sidebar.rs`, `devices.rs`, `drives.rs`, `mount.rs` |
| Clipboard, transfers and name clashes, Trash, rename, undo and redo | `actions.rs` |
| Item and folder menus | `menus.rs` |
| Operation progress (including background operations), reports, archives, leftovers | `operations.rs` |
| Properties, customization, Open With | `properties.rs`, `customize.rs`, `open_with.rs` |
| Search card and filter rows | `search.rs` |
| Appearance menu: view, sort, columns, icon size, density, text size | `appearance.rs` |
| 10xer keys, chords and footer prompts | `tenxer.rs` |
| Keys while the preview drawer has focus | `preview_keys.rs` |
| Folder peeking | `peek.rs` |
| File and folder icons from the icon theme | `icons_theme.rs` |
| Settings | `settings.rs` |
| The portal's file chooser | `chooser.rs`, `chooser/convert.rs` |

The views of a tab share an `EntryModel` (a `gio::ListStore` behind a sort model and a
`MultiSelection`); Columns keeps one per browser column. A tab refreshes from browser events once
per main-loop turn and updates rows in place: GTK does not rebind a row whose item object is
unchanged, so changed rows are spliced in as new objects. Events that only report selection, focus,
previews or peeks do not refresh. Selection made in a view is pushed to the browser; the browser's
selection is mirrored back only when the view has none. Folders opened and finished loading are
recorded in `NavigationHistory` for Jump to Folder. Each tab forwards its window's presence (the
active tab the window's, others hidden), power saver and the Watch network folders preference to
`Browser::set_freshness`, and binds the default sort, per-folder sorts and their revision.

Keys are read through `ui::keys` and matched against catalog bindings (see
[keyboard shortcuts](keyboard-shortcuts.md)). The window's capture-phase handler first gives keys to
the preview drawer when it has focus (`preview_keys.rs`), then to 10xer mode when it is on
(`tenxer.rs`, see [10xer mode](10xer-mode.md)); the bubble-phase handler takes window and file
commands, and nothing acts on the listing while a text field or the drawer has focus. 10xer
browsing keys (motion, find, filter, sort, the **g** chord) are shared with the chooser; file
commands, the copy, actions and tabs chords need a window.

Dialogs are `nyx_ui::widgets::dialog` cards, menus `nyx_ui::menu`, notices `nyx_ui::toast`.

The chooser (`chooser.rs`) is a tab over `ui::chooser::ChooserFileSource`, which applies the
request's filter and folder-only rule, inside a window with the same header, sidebar and views. It
registers as the portal's `ChooserUi`; request validation stays in `portal.rs`, and accepting checks
names, destinations and overwrites through the portal's helpers.

`src/ui` holds the parts the interface builds on: preferences (`ui/preferences.rs`), key bindings
(`ui/keys`), the preview drawer and its renderers (`ui/preview`), thumbnails (`ui/thumbnail`), the
shortcut footer and reference, custom actions and jobs, device and drive helpers, and the theme
manager, which now only supplies colours to previews and syntax highlighting and installs a user
stylesheet from Files' config folder.

### File operations

Every local operation that puts an item at a name goes through `placement::place` in
`adapters/local_operations/placement.rs`, a no-clobber ladder that adapts to what the mount
supports. Its syscalls go through the `FsOps` seam in `fs_ops.rs`, and what each mount supports is
cached in `adapters/volume/mount_capabilities.rs`. See
[Filesystem compatibility](filesystem-compatibility.md).

The names a mount stores are derived from mountinfo, udev and `statfs` in
`adapters/volume/names.rs` and `mounts.rs`, and cached with its capabilities;
`adapters/local_operations/mount_profile.rs` reads them once per operation, so its planner and
workers agree. Its sibling `names.rs` fixes, plans and numbers names by those rules for copies,
moves, merges and extraction; `transfer_targets.rs` works out, off the main thread, the name each
pasted item will get and whether something has it, for the conflict prompt; and `typed_names.rs`
refuses a typed name the location would store as another.

Copies between native paths, including moves across filesystems, merges, Replace stages and
restore copies, go through `adapters/local_operations/copy_engine.rs`. It copies through
descriptors on worker threads, on the queue `work_queue.rs` shares with deletions, and records
what a move read so that only unchanged sources are deleted. `owed.rs` batches the deletions a
paste owes behind one durability barrier per destination filesystem, and the completion report
reaches the UI as `OperationEvent::ItemsReported`. Replacement/merge, deletion, restore/undo and
progress orchestration live in their adjacent `replacement`, `deletion`, `restore` and `progress`
modules. `native_fs` owns shared descriptor primitives, the per-mount identity rule (`compare`)
and `Pinned`, the `O_PATH` hold for items nothing else proves; the syscall seam composes
`FsEnvironment` for Trash, mount-table, udev and `/proc` holder queries without a second injection
registry. Identities that outlive an operation (Trash items, saved views of trashed items, a
folder's load stamp) are `services/item_identity.rs`'s `ItemIdentity`, which carries its mount's
traits and compares without I/O.

Every mutating `OperationProvider` entry, Empty Trash and a drive's unmount hold an
`ActiveOperation` (`adapters/local_operations/release.rs`) until their final event. It registers
the operation in `services/file_activity.rs`, the in-memory registry of Files' own reads and
running operations, and releases what the operation changes: it fences those paths against new
reads, stops the registered reads under them (previews, thumbnails, sandboxes, metadata probes and
background listings) and waits for them within a budget. Readers register at the funnels they
already share, and preview drawers and thumbnails follow the registry's notices on the main thread.
`leftovers.rs` removes folders that hold only other programs' silly-rename leftovers, and
`holders.rs` names the program that still has a file open; see
[Files that are still open](filesystem-compatibility.md#files-that-are-still-open).

Every hidden name Files creates in a user's folder, and every one earlier builds created, is in
the pure table `model/hidden_names.rs`, which adapters take their names from and which classifies a
listed name by its whole form. `app/browser/leftovers.rs` classifies a settled column's names in
memory, hides them while `file_activity` shows an operation in that folder, and emits
`LeftoversChanged` for the notice that offers a review. The review dialog
(`interface/operations.rs`) reads the items through `OperationProvider::review_leftovers`, and
`adapters/local_operations/leftovers.rs` restores originals (`recover`, behind the same
`ActiveOperation` as every operation) and sweeps stale probes; see
[Leftovers after a crash or unplug](filesystem-compatibility.md#leftovers-after-a-crash-or-unplug).

Filesystem work for Trash lives in `adapters/trash.rs`. Measurement shares one entry/time budget
across root and descendant batches; depth truncation and unreadable descendants remain branch-local.
Deleting Trash streams its own batches, independently of any incomplete measurement. Native path
and GIO URI conversion lives in `adapters/gio_location.rs`, shared by files, operations, preview and
browser presentation. It preserves native bytes, keeps GLib's normalized percent-encoding (so names
that are not valid UTF-8 and encoded slashes survive), and sanitizes credentials on inbound GIO
locations.

Local archive operations live under `adapters/local_operations/archive/`:

| Responsibility | Owner |
| --- | --- |
| Operation entry points, worker lifecycle and progress events | `archive.rs` in the parent directory |
| Staged publication, source traversal and compression writers | `compression.rs` |
| Per-operation extraction state, copying, cleanup, size preflight, hard-link resolution, deferred directory metadata and outcomes | `extraction.rs` |
| Confined destination writes, link creation, mode and time restoration, path validation, staging folder lifecycle, publication, member names by the destination's rules and conflict naming | `destination.rs` |
| ZIP, TAR/gzip and 7z member enumeration, passwords and decoder errors | `decoders.rs` |

Every decoder feeds one `ExtractionSession` per operation. The session has no codec or widget
API dependencies; decoders lend it member streams and the archive name, and provide
already-known pending names on cancellation. The session writes members into a hidden staging
folder under the destination and publishes it in `finish` for every outcome: a single root moves
up verbatim, several roots are renamed to the archive stem, and failed or cancelled output stays
in the archive-named folder unless it holds only directories.
A password failure instead discards the staging folder, since the retry extracts everything
again. Decoders report it as `ArchiveError::PasswordRequired` or `IncorrectPassword`, and its kind
travels as `PasswordFailure` on `OperationEvent::Failed` and `BrowserEvent::OperationFailed`;
that kind alone decides the password retry, never the message text. Only a member that is
encrypted and was given a password reports malformed data as a possible wrong password. Staging
that cannot be discarded turns the failure into an ordinary one.
A directory member repeated in the archive is restored once, each field from the last entry
that stores it.
Member identity tracking stays
inside each decoder rather than assuming unique names or matching header/callback order. The
session validates pending names and maps them through the folders it made, fixing the rest by the
destination's rules, without filesystem probes or name reservations; final leaf conflicts remain
unknown until a member is attempted. Sequential formats do not scan unread content to complete
that list.
Before writing, the session checks claimed uncompressed size against destination free space from
`fstatvfs` on the pinned root, and it refuses a member whose extracted size does not match the
size declared by the archive header. ZIP and 7z advertise a total up front, so an oversized
archive is refused before any member is written; TAR streams check each member as it arrives, so
extraction stops at the free-space boundary and members already written stay inside the
archive-named folder, which the failure message names. The
guarantee is that extraction never exceeds the free space observed when the session opened;
it does not model per-file overhead such as block rounding or inodes. Filesystems that report no
capacity (`f_blocks == 0`, as FUSE mounts without `statfs` do) skip the free-space checks and
keep only the declared-size match. The `zip` crate does not bound inflated output by the header
size itself, so that match is the control that stops a ZIP member lying about its size.

Decoders pass symlinks, TAR hard links and each member's mode and modification time through
`MemberContent` and `MemberMetadata`, and refuse FIFOs and device nodes. Restoring metadata is
best effort: `EPERM`, `EOPNOTSUPP` and `EINVAL` from filesystems without Unix permissions or times
are ignored in `destination.rs`, which makes those calls through the `FsOps` seam. TAR extraction
preserves native path bytes where the destination stores them, and hard-link target identity
through fixed names. The sandboxed RAR helper streams
`STRRAR03` records carrying each member's mode and
time (a RAR 5 FILETIME, or the DOS local time of older formats, which only the parent can
convert in the user's zone); RAR links are not yet extracted as links. Error records and failed
member trailers carry a failure kind, so a missing or incorrect password reaches the parent
without parsing text. Format
libraries remain behind the adapter boundary. See [archive creation](archives.md) for
container-specific encoding, classification and cancellation behavior. Archive unit tests sit in each module's
adjacent `tests.rs`; provider-level tests remain in `archive/tests.rs`, with shared test-only builders
in `fixtures.rs`.

Feature unit tests sit beside their implementations. GTK tests that need independent
initialization can use
`test_support::gtk_test`, which launches a subprocess with disposable XDG directories. Set
`STRATA_REQUIRE_GTK_TESTS=1` when exercising those tests on a display to make unavailable GTK a
failure rather than a skip.

Code on the GTK main thread makes no file call, except those listed under [Off the interface
thread](filesystem-compatibility.md#off-the-interface-thread). `util/ui_thread.rs` marks that
thread, and the syscall seam, `FsEnvironment`, the probes, `MountTable::current()` and the settings
writer's file calls assert that they run elsewhere; only named, scoped exceptions (the reads that
build the first window, the exit flush) let them through. The interface asks instead:
`adapters/local_operations/probes.rs` for presence, folder checks, writable destinations and
typed-path listings; `adapters/volume/mount_watch.rs`, a thread that keeps the mount table published
for readers that do no I/O; `adapters/volume/mount_lanes.rs`, which bounds the folder loads, probes,
monitors and other requests it admits to a mount that stops answering;
`adapters/local_files/monitors.rs`, the one worker that creates and cancels file monitors;
`adapters/gio_close.rs`, which closes the enumerators and streams the interface lets go of in the
background (GLib still closes a GVfs stream whose open succeeded just as it was cancelled); and `storage/writer.rs`, the background writer for Files' own settings and state. See
[Unresponsive mounts](filesystem-compatibility.md#unresponsive-mounts).

### Browser directory-event routing

`app/browser/loading.rs` dispatches provider events through an owned open-load target:
native batches stage for sorting/publication, while remote batches keep the first-batch
and coalesced-tail paths. Completion carries truncation and both filesystem capabilities
together. Requests outside the open-load gate retain their existing peek/failure handling;
stale work is still checked against the owning directory or peek request.

A load's `DirectoryEvent::Stamp` arrives before its first batch: it requests the column's change
monitor, which `directory_changes.rs` checks once it is live, and becomes the column's baseline for
`app/browser/freshness.rs` when the listing is shown. That scheduler checks the visible folders that
other writers can change, or that have no live monitor, through `FileSource::check_stamps`, one
background job per mount, and re-lists a moved one through a background request that keeps the
listing if it fails or falls short. A check that finds the folder gone or replaced feeds the same
departure check as a monitor's removal report. Its inputs come from the interface: focus,
visibility and power saver from `ui/presence.rs`, which follows each window, and the "Watch network
folders" preference. It also follows each column's mount and emits `ColumnNotResponding` for the
notice. See
[Changes made elsewhere](filesystem-compatibility.md#changes-made-elsewhere).

`loading/metadata.rs` separates full-sort fills, applied by location, from viewport fills,
validated against directory identity and row-position/location tokens. Metadata chunks
never complete a sort; `MetadataFinished` retains that responsibility. Both modules release
state and routing borrows before synchronous observer dispatch, allowing observers to
navigate safely. Sorting, metadata scheduling, and cancellation remain in the browser
controller; event routing does not change those policies.

A folder's own directory monitor reports its deletion, its rename, and its move alike, as
the removal of the folder itself. A column whose parent column is its parent folder leaves
that report to the parent column's monitor, which sees the departure as a change to one of
its entries; any other column, including the depth-0 folder, is checked in
`directory_changes.rs`. A folder that validates again (deleted and recreated) reloads in
place, which renews its monitor. A missing depth-0 folder is looked up in its parent by
the identity its load's change stamp recorded off the main thread (an `ItemIdentity`),
compared by the mount's [identity rule](filesystem-compatibility.md#identity): a birth time
keeps a new folder that reuses the inode from passing for a rename, and on mounts that
renumber folders without reporting their birth times (sshfs, exFAT) nothing proves a
folder, so it counts as departed. Found there
(`find_by_identity`), the folder was renamed in place: the search indexes are rebased and
the open columns relocate (`ColumnsRelocated`), as for an in-app rename. Not found, with a
parent that cannot be listed, or before its identity is known, it departed: the nearest
existing ancestor that the source allows is restored without a history entry, or the
folder shows its read error when there is none. Backends without identities treat every
rename as a departure. A folder or ancestor that cannot be reached (unmounted, offline, or
not permitted) stays as it is. A monitor can lose the removal report, when a burst of
changes turns its batch into a rescan or when a reload supersedes the check, so a failed
reload of a depth-0 folder that loaded before runs the same check; it leaves the read
error in place unless the folder is confirmed gone. A departure that arrives during a
deletion, restoration, or transfer waits with the operation's other changes, which apply
first.

### Browser staged publication

`app/browser/publication.rs` owns staged row publication on the same `Browser`, not a
second controller. A `PublicationPlan` captures request identity, row count, selection,
and terminal payload; only the separate progress cursor advances while tails stream.
Sort paths capture the plan before notifying preference observers.

Inline publication, idle completion, and synchronous draining share selection/terminal
dispatch. Selection follows the final rows; a load's metadata retry follows its load
completion event. Idle tails reject superseded directory requests and respect both the
captured total and current model length. Draining instead publishes the entire remaining
current model before mutations that require convergence. Borrows end before synchronous
observer calls, including cancellation from a tail observer.

Inline thresholds, prefix/chunk sizes, idle priority, and the cooperative time budget
are unchanged. Queue ownership and cancellation/truncation remain on `Browser`; sort
workers, remote coalescing, and operation callbacks are separate responsibilities.

### Pins

Bookmark storage lives in `adapters/bookmarks.rs`: every pin, unpin, reorder and deletion cleanup
is one job on the background settings writer (`storage::writer`) that reads the shared GTK file,
applies the change and saves it, checking just before its rename that no other program changed the
file since the read and starting over, at most twice, if one did. Every open sidebar then adopts the
pins the file holds; the sidebar keeps the pins the watcher hands it rather than re-reading the file
while it rebuilds. A symlinked bookmarks file is written through to its target under the same
ownership rules as `settings.toml`.

## Capability boundaries

### File source

Enumerates locations, retrieves metadata, watches changes, identifies folders across renames, and reports supported actions. Every load reports the folder's change stamp (`DirectoryEvent::Stamp`) before its first batch and a digest of the listed names before it finishes; `probe_stamp` and `check_stamps` read a stamp again on request, `check_stamps` as background work grouped per mount, and `responsiveness` says, without I/O, whether a location's mount still answers. Begin with local files. Avoid designing a universal remote filesystem API before a second backend exists.

### Operation service

Owns mutations, progress, cancellation, conflicts, and partial outcomes. UI code submits commands and observes operation state.

### Search provider

Streams scoped results and supports cancellation. Current-list filtering can remain in the application model; recursive filename and content search are providers.

### Preview registry

Chooses providers by content type and declared priority. Every provider receives byte/time/dimension budgets and returns either a preview, an unsupported result, or a contained failure.

### Theme source

Produces semantic tokens. Omarchy, generic system appearance, and user files are sources feeding one validated theme model.

### Settings store

Loads and saves a versioned schema through XDG-standard locations. Unknown keys are tolerated, defaults are centralized, and migrations are explicit.

Application preferences use immediate-and-live bindings rather than separate startup and Settings-page initializers. See [Preference lifecycle](preferences.md) for consumer scopes, binding ownership, and required regression coverage.

## Customization model

Start with stable data-driven customization:

- Semantic color tokens
- Typography tokens for interface and monospace preview text
- Density, spacing, radius, and animation tokens
- Keybinding configuration
- Search exclusions
- Preview enablement and limits

Internally, search, preview, and theme implementations should be registries so built-in providers remain modular. This does **not** require exposing an unsafe public plugin ABI in the first release.

When third-party extensions are justified, prefer a versioned message protocol with explicit capabilities and permissions. This permits extensions written in multiple languages and allows isolation from the main process.

### Custom actions

User-authored context-menu actions follow that direction without an ABI:
`model::action` parses and validates the portable `action.toml` contract,
`services::actions` owns the cached catalog and matching, `adapters::local_actions`
reads, validates, writes, imports, and exports action directories, and
`adapters::local_jobs` starts one invocation as a child process. `services::jobs`
owns the queue, per-item iteration, progress, cancellation bookkeeping, bounded
logs, and history, with the presentation in `ui/jobs.rs` observing it.

Definitions are data, not code, until an action is invoked: opening a menu only
matches declarative rules. Invocations receive their inputs through files and the
environment rather than through a shell or an interpolated command line, and the
registry, job service, and runner keep filesystem, process, and widget
responsibilities apart. Actions are trusted local programs; this boundary is
organizational, not a security sandbox. See [Custom actions](custom-actions.md).

## Suggested source organization

```text
src/
├── app/          # orchestration, commands, state transitions
├── model/        # product models with no widget dependencies
├── services/     # capability contracts and shared request/result types
├── adapters/     # local files, search tools, themes, settings
├── interface/    # the window, the file chooser and their parts
├── ui/           # parts the interface builds on: preferences, keys, previews, thumbnails
└── main.rs       # startup and dependency composition
```

This is a direction, not a requirement to create empty modules. Move code only when the associated responsibility exists.

## Decision records

Significant decisions should be captured as short ADRs under `docs/adr/`, including context, decision, consequences, and status. Appropriate subjects include extension isolation, configuration format, preview sandboxing, and indexed search.
