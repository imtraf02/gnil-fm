# gnil-fm architecture

The workspace keeps UI state separate from filesystem work so the GPUI render thread never performs
recursive I/O.

| Crate | Responsibility |
| --- | --- |
| `gnil-clipboard` | Local file-URI and GNOME/KDE Wayland clipboard MIME encoding/decoding |
| `gnil-core` | Stable models, action IDs, tab history, settings and serializable operation records |
| `gnil-fs` | Directory scans, fuzzy search, watching, Git status, prioritized jobs and safe mutations |
| `gnil-preview` | Bounded text, image, directory and metadata previews |
| `gnil-app` | GPUI file-manager window plus the independent `gnil-fm-portal` picker service |

The main binary is intentionally thin. File-manager behavior is grouped under
`gnil-app/src/file_manager/` by loading, interaction, operation and view concerns. First-party Rust
source files have a hard 2,000-line limit enforced by `nix flake check`; vendored GPUI sources are
excluded.

UI rendering is split further by surface: sidebar, header, appearance, menus, settings, operation
sheets, lists and workspace. The portal picker follows the same loading/actions/view/footer split.
Shared semantic icon, focus and overlay primitives live under `gnil-app/src/ui/`; project-specific
UI changes must follow `.agents/skills/gnil-fm-ui-design/SKILL.md`. UI view modules have an enforced
600-line limit even though the repository-wide safety limit is higher.

## Data flow

1. Navigation advances a generation counter, cancels the previous request and submits a directory
   scan.
2. The scan emits a discovered snapshot containing names and kinds, then a complete snapshot with
   file sizes and direct child counts for folders. Stale generations are discarded.
3. Selection launches a bounded, cancellable preview request. Syntax resources are reused and
   fingerprinted results may come from the bounded memory cache.
4. Mutations run away from the render thread and return an optional undo record.
5. The non-recursive watcher coalesces bursts and refreshes the current directory after external or
   successful local changes.

`TaskScheduler` is available for longer foreground and background work. Jobs have explicit priority,
cancellation and progress events. `DirectoryWatcher` wraps `notify` non-recursively; the app polls it
at a 100 ms debounce boundary and requests a fresh snapshot rather than patching UI rows from raw
events. Directory snapshots are shared with render closures through `Arc`, so virtualized rendering
does not clone all entries on every frame or perform filesystem I/O.

## Safety invariants

- Recursive copies inspect symlinks and never traverse through them.
- A directory cannot be copied or moved beneath itself.
- File copies use a unique `.gnil-part-*` sibling and rename only after a complete flush.
- An existing destination requires an explicit conflict policy.
- Undoing a copy removes a result only when its size and modification fingerprint still match.
- Trash is the default destructive action; permanent deletion is separately confirmed and has no undo.
- Chmod preflights all paths, never follows symlinks and records before/after modes for guarded undo.
- Bulk rename is limited to one directory and uses UUID staging names to support swaps and cycles.
- Text preview stops at 2 MiB and image decoding stops above 50 megapixels.

## Platform boundaries

The application targets local POSIX filesystems on Linux/Wayland. X11, macOS, Windows, GVFS and
remote SMB/NFS discovery are intentionally outside the supported/tested matrix. Clipboard codecs
reject non-local URIs rather than turning them into filesystem paths.

## FileChooser portal

`gnil-fm-portal` owns the implementation-side FileChooser interface on the session bus. Each backend
method installs a temporary `org.freedesktop.impl.portal.Request` object at the supplied handle,
opens a separate GPUI picker, and keeps the method call pending until the picker returns response 0,
1 or 2. The public `org.freedesktop.portal.Request::Response` signal remains the responsibility of
`xdg-desktop-portal`.

The D-Bus executor communicates with the GPUI event loop through channels, so simultaneous callers
never share navigation, selection, filter, choice or filename state. Closing a window, pressing
Escape, clicking Cancel or invoking `Request.Close` all converge on the same exactly-once
completion guard. GPUI is vendored at version 0.2.2 to add xdg-foreign v2 external parenting,
native Wayland `text/uri-list` drag sources and an opt-in session-service keep-alive; normal
`gnil-fm` windows otherwise retain upstream lifecycle behavior.
