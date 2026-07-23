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

## FileChooser portal

`gnil-fm-portal` owns the implementation-side FileChooser interface on the session bus. Each backend
method installs a temporary `org.freedesktop.impl.portal.Request` object at the supplied handle,
opens a separate GPUI picker, and keeps the method call pending until the picker returns response 0,
1 or 2. The public `org.freedesktop.portal.Request::Response` signal remains the responsibility of
`xdg-desktop-portal`.

The D-Bus executor communicates with the GPUI event loop through channels, so simultaneous callers
never share navigation, selection, filter, choice or filename state. Closing a window, pressing
Escape, clicking Cancel or invoking `Request.Close` all converge on the same exactly-once
completion guard. GPUI is vendored at version 0.2.2 solely to add xdg-foreign v2 external parenting
and an opt-in session-service keep-alive; normal `gnil-fm` windows retain upstream lifecycle
behavior.
