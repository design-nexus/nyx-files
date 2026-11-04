# Files

A Rust/GTK4 file manager for the Nyx desktop, with integrated cloud navigation
and selectable preview presentation. Its origin and licenses are in
[UPSTREAM.md](UPSTREAM.md) and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

- Tabs, with Columns, Icons and List views; the sidebar holds places, pins,
  devices and cloud accounts.
- File and folder icons come from the system icon theme (Umbra in the suite);
  thumbnails replace them where available.
- The preview drawer beside the listing shows documents, images, archives,
  audio and video with sandboxed renderers. Settings → Previews → Preview style
  selects Popup (default) or Panel; Space toggles the preview.
- 10xer mode (Ctrl+Shift+M) gives keyboard-first navigation with chords and
  prompts in the shortcut footer; F1 opens the shortcut reference.
- Files can serve as the system file chooser for Open, Save and Select Folder
  dialogs (Settings → Integrations).
- Cloud Storage lists accounts enabled for Storage in Settings → Online Accounts.
  Selecting an account mounts it through Cloud and opens it.
- Settings → Sidebar controls cloud visibility, connections, login mounting,
  cache limits and bandwidth, with a link to Online Accounts for authorization.
- Files continues to manage local files when Cloud is absent.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nyx-files/main/install.sh | bash
```

This installs the latest release; `NYX_BUILD=1` builds from source instead.
`./uninstall.sh` removes Files (`--purge` also removes its settings and data).

## Build and install

Requires Rust, GTK4 4.12+, GtkSourceView5, Poppler GLib, Fontconfig,
GStreamer with app/base development libraries, and the existing preview helpers
(including bubblewrap for isolated native decoding). Install `nyx-cloud`, rclone,
and FUSE3 for cloud features; Settings provides graphical account configuration.

```
./install.sh
```

From a checkout, the installer builds it and installs the binary, launcher and
icon, and makes Files the default app for folders and Trash links. An existing
per-user FileManager1 activation file is set aside before replacement, and
`./uninstall.sh` puts it back. Installation does not launch or restart the application.
Files keeps its settings, state and cache in its own folders.

Public dependencies are pinned to immutable Git revisions. CI builds the app
independently. Tagged x86_64 releases package installation resources and publish
signed update manifests using this repository's dedicated Ed25519 release key.
The private key is never committed; release automation reads UPDATE_SIGNING_KEY.

See [Cloud](https://github.com/design-nexus/nyx-cloud) for mount locations,
configuration security, upload/cache semantics, and service API documentation.
A successful local write can still await cloud upload. Account removal never
deletes cloud data. Quotas, sharing links, offline pinning, and cloud-wide search
are secondary features and are not implemented.

Licensing/provenance: [UPSTREAM.md](UPSTREAM.md), original MIT [LICENSE-MIT](LICENSE-MIT),
GPLv3 [COPYING](COPYING), and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
The combined application includes GPL-3.0-or-later source files. RAR/CBR decoding
uses a separate MIT/UnRAR-licensed helper, included by the installer; its native
code is not linked into the GPL executable.
