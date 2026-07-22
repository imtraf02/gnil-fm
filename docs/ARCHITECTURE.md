# gnil-fm architecture

The workspace keeps UI state separate from filesystem work so the GPUI render thread never performs
recursive I/O.

| Crate | Responsibility |
| --- | --- |
| `gnil-clipboard` | Local file-URI and GNOME/KDE Wayland clipboard MIME encoding/decoding |
| `gnil-core` | Stable models, action IDs, tab history, settings and serializable operation records |
| `gnil-fs` | Directory scans, fuzzy search, watching, Git status, prioritized jobs and safe mutations |
| `gnil-preview` | Bounded text, image, directory and metadata previews |
| `gnil-app` | GPUI Wayland window, multi-selection, operation sheets and user confirmation |

## Data flow

1. Navigation advances a generation counter and submits a directory scan.
2. The scan returns an immutable snapshot; stale generations are discarded.
3. Selection launches a bounded preview request. A result is accepted only if its path still matches.
4. Mutations run away from the render thread and return an optional undo record.
5. The UI refreshes the current directory after a successful mutation.

`TaskScheduler` is available for longer foreground and background work. Jobs have explicit priority,
cancellation and progress events. `DirectoryWatcher` wraps `notify` non-recursively; consumers should
debounce bursts and request a fresh snapshot rather than patching UI rows from raw events.

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
