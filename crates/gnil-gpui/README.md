# gnil-gpui

`gnil-gpui` is the first-party UI runtime used by gnil-fm. The Rust library remains named `gpui`
so application code can use the familiar `gpui::` namespace.

The crate is a hard fork derived from Zed Industries' GPUI 0.2.2. It is intentionally scoped to
Linux/Wayland and to the components used by gnil-fm: views, `Div`, `UniformList`, overlays, text,
local images, SVG, prompts, clipboard, keyboard actions and the test platform.

The fork also carries gnil-fm's xdg-foreign v2 parent handle, native outbound file drag and explicit
event-loop quit policy. macOS, Windows, X11, remote image loading, file dialogs, URL opening,
credentials, application menus, screen capture, inspector support and dynamic JSON action
registration are outside this crate's contract.

Unsafe Rust is denied by default and permitted only inside the arena/executor, Taffy adapter,
Blade renderer and Wayland FFI boundary modules.

The original GPUI source is licensed under Apache-2.0. See `LICENSE-APACHE`.
