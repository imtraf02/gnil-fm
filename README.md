# gnil-fm

`gnil-fm` is a native Rust file manager for Linux/Wayland. Its interface borrows the calm density
and command model of Zed, while its filesystem engine separates browsing, background jobs, and
bounded previews.

## Current MVP

- GPUI Wayland shell with a virtualized file list, Quick Access sidebar and adaptive preview panel
- ordered folder Favorites with hover actions, context-menu toggles and missing-path recovery
- keyboard navigation, history, hidden-file toggle, system opener and searchable XDG “Open with”
- multi-select copy/cut/paste, native Wayland file drag-out, Copy Path, Trash, permanent deletion
  confirmation and guarded undo
- relative/absolute symlink creation, non-recursive chmod and cycle-safe bulk rename with live preview
- non-blocking directory scans and preview generation
- text/code highlighting, image preview and metadata fallback with hard safety limits
- cancellable priority scheduler, recursive current-folder and Home search, metadata-rich directory
  scans and filesystem watcher
- safe copy/move/create/rename/trash/permanent-delete engine with conflict policies and session undo
- staged, cancellable extraction for ZIP, TAR, 7z, RAR and common compressed streams
- XDG configuration, Nix dev shell/package and Linux desktop metadata
- D-Bus-activatable FileChooser portal backend with read-only Open, Save and SaveFiles dialogs

## Develop

```sh
nix develop path:.
cargo test --workspace
cargo run -p gnil-fm -- ~/Downloads
```

The UI targets native Wayland. GPU, fontconfig, FreeType and xkbcommon libraries are supplied by the
Nix shell. Use `cargo run --profile profiling -p gnil-fm -- ~/Downloads` for a release-equivalent
build with symbols. Set `GNIL_PERF_TRACE=1` when launching the app to print one-second summaries of
dispatch, draw, submit and input-to-submit latency, missed frame budgets, surface invalidations,
visible row work and coalesced pointer latency.

For a one-shot launch without entering the development shell first:

```sh
nix develop -c env GNIL_PERF_TRACE=1 cargo run --profile profiling -p gnil-fm -- ~/Downloads
```

## Keyboard

| Key | Action |
| --- | --- |
| `↑` / `↓` | Move the cursor |
| `Shift+↑` / `Shift+↓` | Extend selection |
| `Ctrl+Space` | Toggle the cursor item in the selection |
| `Enter` | Open file or enter folder |
| `Space` | Toggle preview |
| `Alt+←` / `Alt+→` | Back / forward |
| `Alt+↑` | Parent folder |
| `Ctrl+H` | Toggle hidden files |
| `Ctrl+D` | Add or remove the selected folder from Favorites |
| `F5` | Refresh |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste |
| `Ctrl+Shift+C` / `Ctrl+Alt+C` | Copy absolute / relative paths |
| `F2` | Rename one item or open Bulk Rename |
| `Ctrl+Shift+L` | Create a symlink in the current folder |
| `Alt+Enter` | Edit POSIX permissions, non-recursively |
| `Ctrl+E` / `Ctrl+Shift+E` | Extract beside the archive / choose a destination |
| `Delete` | Move the selection to Trash |
| `Shift+Delete` | Permanently delete after confirmation |
| `Ctrl+Z` | Undo the latest reversible file operation |
| `Ctrl+Shift+T` | Open the appearance and theme menu |

Configuration is read from `$XDG_CONFIG_HOME/gnil-fm/config.toml`; absent files use safe defaults.

### Themes

Appearance mode and the selected theme for each mode are persisted in `config.toml`:

```toml
theme = "system" # system, light, or dark
light_theme = "GNIL Light"
dark_theme = "Forest Night"
```

Custom themes are JSON files in `$XDG_CONFIG_HOME/gnil-fm/themes/`. Colors that are omitted inherit
from the built-in palette for that theme's `appearance`, so a theme may override only the tokens it
needs. Invalid files are skipped without preventing the application from starting; the Appearance
menu shows the error count and provides a Reload action. See
[`themes/forest-night.json`](themes/forest-night.json) for the complete version-1 schema.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for package boundaries and concurrency rules.

### Custom key bindings

Open **Settings → Keymap & Controls** or press `Ctrl+K Ctrl+S` to search and customize
file-manager commands. The default keymap remains active while overrides apply immediately. The
editor can add chords, change or unbind a default key, reset one command, reset everything, and
reload manual edits. You can add single-letter navigation or file-operation bindings if that suits
your workflow.

Overrides are stored in `$XDG_CONFIG_HOME/gnil-fm/keymap.toml`:

```toml
version = 1

[[bindings]]
action = "file.copy"
keystrokes = "ctrl-k ctrl-c"
kind = "bind"

[[bindings]]
action = "file.copy"
keystrokes = "ctrl-c"
kind = "unbind"
```

Use the **Reload** button after editing this file outside gnil-fm. Invalid files leave the active
keymap unchanged and show an error in the editor.

## Build and package

```sh
nix build path:.
nix build path:.#tarball
```

The default output is an installable Nix package with desktop metadata. `#tarball` produces a
self-contained, architecture-specific Linux archive with its dynamic loader, runtime libraries and
shared assets; GPU drivers still come from the host system.

For a NixOS system installation:

```nix
# flake.nix
inputs.gnil-fm = {
  url = "github:imtraf02/gnil-fm";
  inputs.nixpkgs.follows = "nixpkgs";
};

# A NixOS module
imports = [ inputs.gnil-fm.nixosModules.default ];
programs.gnil-fm.enable = true;
programs.gnil-fm.portal.enable = true; # opt in as Niri's FileChooser backend
```

For a per-user Home Manager installation and optional default directory handler:

```nix
imports = [ inputs.gnil-fm.homeManagerModules.default ];
programs.gnil-fm = {
  enable = true;
  defaultFileManager = true;
  portal.enable = true;
};
```

`defaultFileManager` and `portal.enable` are off by default. The portal option selects
`gnilfm;gtk;` for `org.freedesktop.impl.portal.FileChooser`, leaving GTK as a fallback. The Home
Manager option owns `xdg-desktop-portal/niri-portals.conf`; keep it disabled if that file is managed
elsewhere. Log out and back in after changing portal selection so the session services restart.

### Install in `nixos-minimal`

The [`imtraf02/nixos-minimal`](https://github.com/imtraf02/nixos-minimal) configuration already
declares the `gnil-fm` flake input and imports `inputs.gnil-fm.homeManagerModules.default` from
`home/imtraf/default.nix`. Enable the portal beside the existing default-file-manager option:

```nix
programs.gnil-fm = {
  enable = true;
  defaultFileManager = true;
  portal.enable = true;
};
```

Then rebuild the repository's laptop target and start a fresh graphical session:

```sh
sudo nixos-rebuild switch --flake .#nixos-laptop
```

After logging out and back in, verify the selected backend and its activation metadata:

```sh
cat ~/.config/xdg-desktop-portal/niri-portals.conf
systemctl --user status xdg-desktop-portal.service
busctl --user introspect \
  org.freedesktop.impl.portal.desktop.gnilfm \
  /org/freedesktop/portal/desktop
```

The configuration file should select `gnilfm;gtk;` for
`org.freedesktop.impl.portal.FileChooser`, and the introspection output should show `OpenFile`,
`SaveFile`, `SaveFiles`, and version `4`. If activation fails, inspect both portal processes:

```sh
journalctl --user -b \
  -u xdg-desktop-portal.service \
  -u xdg-desktop-portal-gnilfm.service
```

The backend executable is `gnil-fm-portal` and owns
`org.freedesktop.impl.portal.desktop.gnilfm`. It is activated by D-Bus and can serve independent,
concurrent picker windows even when the main file-manager window is not running. Picker windows are
read-only: no rename, delete, paste, filesystem drag-and-drop, Trash or terminal actions are
registered. On Wayland, `wayland:<handle>` parents are attached with xdg-foreign v2; compositors
without that protocol receive an independent toplevel as a safe fallback.

The backend methods follow the implementation-side portal contract: the temporary
`org.freedesktop.impl.portal.Request` object accepts `Close`, while the method returns the response
code and results after interaction. The public `xdg-desktop-portal` service owns the caller-facing
request object and emits `org.freedesktop.portal.Request.Response`; callers should use the public
service rather than invoking this backend directly. Closing with Cancel, Escape, or the window close
button resolves as user cancellation. Window creation failures resolve as an error instead of
leaving the caller waiting.

This package currently installs the main app and FileChooser backend as separate binaries and
services. A separate `org.freedesktop.FileManager1` service is not provided yet.

D-Bus and systemd activation metadata is wired automatically by the Nix package. The portable
tarball includes a runnable `gnil-fm-portal` launcher, but selecting it as the session backend still
requires installing activation metadata with paths matching the tarball's final install location.

## Safety model

Symlinks are never followed by recursive operations. Copies are written to `.gnil-part-*` files and
renamed only after a successful flush. Existing files are never overwritten without an explicit
conflict decision. Chmod rejects symlinks and is non-recursive. Bulk rename stages every source under
a unique same-directory name so swaps and cycles are rollback-safe. Archive extraction rejects path
escapes, special nodes and unsafe links, stages the complete batch, and commits with no-replace
renames. Permanent deletion has no undo.

Licensed under the MIT License.
