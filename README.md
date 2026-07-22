# gnil-fm

`gnil-fm` is a native Rust file manager for Linux/Wayland. Its interface borrows the calm density
and command model of Zed, while its filesystem engine follows Yazi's separation between browsing,
background jobs, and bounded previews.

## Current MVP

- GPUI Wayland shell with a virtualized file list, Places sidebar and adaptive preview panel
- keyboard navigation, history, hidden-file toggle and system opener
- multi-select copy/cut/paste, Copy Path, Trash, permanent deletion confirmation and guarded undo
- relative/absolute symlink creation, non-recursive chmod and cycle-safe bulk rename with live preview
- non-blocking directory scans and preview generation
- text/code highlighting, image preview and metadata fallback with hard safety limits
- cancellable priority scheduler, fuzzy path search, filesystem watcher and Git status service
- safe copy/move/create/rename/trash/permanent-delete engine with conflict policies and session undo
- staged, cancellable extraction for ZIP, TAR, 7z, RAR and common compressed streams
- XDG configuration, Nix dev shell/package and Linux desktop metadata

## Develop

```sh
nix develop path:.
cargo test --workspace
cargo run -p gnil-fm -- ~/Downloads
```

The UI targets native Wayland. GPU, fontconfig, FreeType and xkbcommon libraries are supplied by the
Nix shell.

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

Set `keymap = "yazi"` to enable `j/k/l/h` navigation, Space multi-selection and `y/x/p/d/u`
file actions. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for package boundaries and concurrency rules.

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
imports = [ inputs.gnil-fm.nixosModules.default ];
programs.gnil-fm.enable = true;
```

For a per-user Home Manager installation and optional default directory handler:

```nix
imports = [ inputs.gnil-fm.homeManagerModules.default ];
programs.gnil-fm = {
  enable = true;
  defaultFileManager = true;
};
```

`defaultFileManager` is off by default; enabling the module alone never changes MIME preferences.

## Safety model

Symlinks are never followed by recursive operations. Copies are written to `.gnil-part-*` files and
renamed only after a successful flush. Existing files are never overwritten without an explicit
conflict decision. Chmod rejects symlinks and is non-recursive. Bulk rename stages every source under
a unique same-directory name so swaps and cycles are rollback-safe. Archive extraction rejects path
escapes, special nodes and unsafe links, stages the complete batch, and commits with no-replace
renames. Permanent deletion has no undo.

Licensed under the MIT License.
