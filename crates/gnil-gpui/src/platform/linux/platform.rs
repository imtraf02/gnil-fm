#![allow(unsafe_code)]

use std::{env, path::PathBuf, process::Command, rc::Rc, sync::Arc};
#[cfg(any(feature = "wayland", feature = "x11"))]
use std::{
    ffi::OsString,
    io::{self, Read as _},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use async_task::Runnable;
use calloop::{LoopSignal, channel::Channel};
#[cfg(any(feature = "wayland", feature = "x11"))]
use xkbcommon::xkb::{self, Keycode, Keysym, State};

use crate::{
    AnyWindowHandle, BackgroundExecutor, ClipboardItem, CursorStyle, DisplayId, ForegroundExecutor,
    LinuxDispatcher, Pixels, Platform, PlatformDisplay, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Point, QuitPolicy,
    WindowAppearance, WindowParams, px,
};

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) const SCROLL_LINES: f32 = 3.0;

// Values match the defaults on GTK.
// Taken from https://github.com/GNOME/gtk/blob/main/gtk/gtksettings.c#L320
#[cfg(any(feature = "wayland", feature = "x11"))]
pub(crate) const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);
pub(crate) const DOUBLE_CLICK_DISTANCE: Pixels = px(5.0);
#[cfg(any(feature = "wayland", feature = "x11"))]
pub trait LinuxClient {
    fn compositor_name(&self) -> &'static str;
    fn with_common<R>(&self, f: impl FnOnce(&mut LinuxCommon) -> R) -> R;
    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout>;
    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>>;
    #[allow(unused)]
    fn display(&self, id: DisplayId) -> Option<Rc<dyn PlatformDisplay>>;
    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>>;
    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>>;
    fn set_cursor_style(&self, style: CursorStyle);
    fn write_to_primary(&self, item: ClipboardItem);
    fn write_to_clipboard(&self, item: ClipboardItem);
    fn read_from_primary(&self) -> Option<ClipboardItem>;
    fn read_from_clipboard(&self) -> Option<ClipboardItem>;
    fn active_window(&self) -> Option<AnyWindowHandle>;
    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>>;
    fn run(&self);
}

#[derive(Default)]
pub(crate) struct PlatformHandlers {
    pub(crate) quit: Option<Box<dyn FnMut()>>,
    pub(crate) reopen: Option<Box<dyn FnMut()>>,
    pub(crate) keyboard_layout_change: Option<Box<dyn FnMut()>>,
}

pub(crate) struct LinuxCommon {
    pub(crate) background_executor: BackgroundExecutor,
    pub(crate) foreground_executor: ForegroundExecutor,
    pub(crate) text_system: Arc<dyn PlatformTextSystem>,
    pub(crate) appearance: WindowAppearance,
    pub(crate) auto_hide_scrollbars: bool,
    pub(crate) callbacks: PlatformHandlers,
    pub(crate) signal: LoopSignal,
    pub(crate) quit_policy: QuitPolicy,
}

impl LinuxCommon {
    pub fn new(signal: LoopSignal) -> (Self, Channel<Runnable>) {
        let (main_sender, main_receiver) = calloop::channel::channel::<Runnable>();

        #[cfg(any(feature = "wayland", feature = "x11"))]
        let text_system = Arc::new(crate::CosmicTextSystem::new());
        #[cfg(not(any(feature = "wayland", feature = "x11")))]
        let text_system = Arc::new(crate::NoopTextSystem::new());

        let callbacks = PlatformHandlers::default();

        let dispatcher = Arc::new(LinuxDispatcher::new(main_sender));

        let background_executor = BackgroundExecutor::new(dispatcher.clone());

        let common = LinuxCommon {
            background_executor,
            foreground_executor: ForegroundExecutor::new(dispatcher),
            text_system,
            appearance: WindowAppearance::Light,
            auto_hide_scrollbars: false,
            callbacks,
            signal,
            quit_policy: QuitPolicy::LastWindowClosed,
        };

        (common, main_receiver)
    }
}

impl<P: LinuxClient + 'static> Platform for P {
    fn background_executor(&self) -> BackgroundExecutor {
        self.with_common(|common| common.background_executor.clone())
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.with_common(|common| common.foreground_executor.clone())
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.with_common(|common| common.text_system.clone())
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.keyboard_layout()
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        Rc::new(crate::DummyKeyboardMapper)
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| common.callbacks.keyboard_layout_change = Some(callback));
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        on_finish_launching();

        LinuxClient::run(self);

        let quit = self.with_common(|common| common.callbacks.quit.take());
        if let Some(mut fun) = quit {
            fun();
        }
    }

    fn quit(&self) {
        self.with_common(|common| common.signal.stop());
    }

    fn set_quit_policy(&self, policy: QuitPolicy) {
        self.with_common(|common| common.quit_policy = policy);
    }

    fn compositor_name(&self) -> &'static str {
        self.compositor_name()
    }

    fn restart(&self, binary_path: Option<PathBuf>) {
        use std::os::unix::process::CommandExt as _;

        // get the process id of the current process
        let app_pid = std::process::id().to_string();
        // get the path to the executable
        let app_path = if let Some(path) = binary_path {
            path
        } else {
            match self.app_path() {
                Ok(path) => path,
                Err(err) => {
                    log::error!("Failed to get app path: {:?}", err);
                    return;
                }
            }
        };

        log::info!("Restarting process, using app path: {:?}", app_path);

        // Script to wait for the current process to exit and then restart the app.
        let script = format!(
            r#"
            while kill -0 {pid} 2>/dev/null; do
                sleep 0.1
            done

            {app_path}
            "#,
            pid = app_pid,
            app_path = app_path.display()
        );

        #[allow(
            clippy::disallowed_methods,
            reason = "We are restarting ourselves, using std command thus is fine"
        )]
        let restart_process = Command::new("/usr/bin/env")
            .arg("bash")
            .arg("-c")
            .arg(script)
            .process_group(0)
            .spawn();

        match restart_process {
            Ok(_) => self.quit(),
            Err(e) => log::error!("failed to spawn restart script: {:?}", e),
        }
    }

    fn activate(&self, _ignoring_other_apps: bool) {
        log::info!("activate is not implemented on Linux, ignoring the call")
    }

    fn hide(&self) {
        log::info!("hide is not implemented on Linux, ignoring the call")
    }

    fn hide_other_apps(&self) {
        log::info!("hide_other_apps is not implemented on Linux, ignoring the call")
    }

    fn unhide_other_apps(&self) {
        log::info!("unhide_other_apps is not implemented on Linux, ignoring the call")
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.primary_display()
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.displays()
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.active_window()
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        self.window_stack()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        self.open_window(handle, options)
    }

    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| {
            common.callbacks.quit = Some(callback);
        });
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.with_common(|common| {
            common.callbacks.reopen = Some(callback);
        });
    }

    fn app_path(&self) -> Result<PathBuf> {
        // get the path of the executable of the current process
        let app_path = env::current_exe()?;
        Ok(app_path)
    }

    fn path_for_auxiliary_executable(&self, _name: &str) -> Result<PathBuf> {
        Err(anyhow::Error::msg(
            "Platform<LinuxPlatform>::path_for_auxiliary_executable is not implemented yet",
        ))
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.set_cursor_style(style)
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        self.with_common(|common| common.auto_hide_scrollbars)
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.with_common(|common| common.appearance)
    }

    fn write_to_primary(&self, item: ClipboardItem) {
        self.write_to_primary(item)
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.write_to_clipboard(item)
    }

    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.read_from_primary()
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.read_from_clipboard()
    }
}

#[allow(unused)]
pub(super) fn is_within_click_distance(a: Point<Pixels>, b: Point<Pixels>) -> bool {
    let diff = a - b;
    diff.x.abs() <= DOUBLE_CLICK_DISTANCE && diff.y.abs() <= DOUBLE_CLICK_DISTANCE
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) fn get_xkb_compose_state(cx: &xkb::Context) -> Option<xkb::compose::State> {
    let mut locales = Vec::default();
    if let Some(locale) = env::var_os("LC_CTYPE") {
        locales.push(locale);
    }
    locales.push(OsString::from("C"));
    let mut state: Option<xkb::compose::State> = None;
    for locale in locales {
        if let Ok(table) =
            xkb::compose::Table::new_from_locale(cx, &locale, xkb::compose::COMPILE_NO_FLAGS)
        {
            state = Some(xkb::compose::State::new(
                &table,
                xkb::compose::STATE_NO_FLAGS,
            ));
            break;
        }
    }
    state
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) fn read_fd_bounded(
    mut fd: filedescriptor::FileDescriptor,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Vec<u8>> {
    fd.set_non_blocking(true)?;
    let started = Instant::now();
    let mut buffer = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut chunk = [0_u8; 64 * 1024];

    loop {
        if started.elapsed() >= timeout {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "data transfer timed out").into());
        }
        let remaining = max_bytes.saturating_sub(buffer.len());
        let read_len = remaining.saturating_add(1).min(chunk.len());
        match fd.read(&mut chunk[..read_len]) {
            Ok(0) => return Ok(buffer),
            Ok(read) if read > remaining => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("data transfer exceeds {max_bytes} bytes"),
                )
                .into());
            }
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) const DEFAULT_CURSOR_ICON_NAME: &str = "left_ptr";

impl CursorStyle {
    #[cfg(any(feature = "wayland", feature = "x11"))]
    pub(super) fn to_icon_names(self) -> &'static [&'static str] {
        // Based on cursor names from chromium:
        // https://github.com/chromium/chromium/blob/d3069cf9c973dc3627fa75f64085c6a86c8f41bf/ui/base/cursor/cursor_factory.cc#L113
        match self {
            CursorStyle::Arrow => &[DEFAULT_CURSOR_ICON_NAME],
            CursorStyle::IBeam => &["text", "xterm"],
            CursorStyle::Crosshair => &["crosshair", "cross"],
            CursorStyle::ClosedHand => &["closedhand", "grabbing", "hand2"],
            CursorStyle::OpenHand => &["openhand", "grab", "hand1"],
            CursorStyle::PointingHand => &["pointer", "hand", "hand2"],
            CursorStyle::ResizeLeft => &["w-resize", "left_side"],
            CursorStyle::ResizeRight => &["e-resize", "right_side"],
            CursorStyle::ResizeLeftRight => &["ew-resize", "sb_h_double_arrow"],
            CursorStyle::ResizeUp => &["n-resize", "top_side"],
            CursorStyle::ResizeDown => &["s-resize", "bottom_side"],
            CursorStyle::ResizeUpDown => &["sb_v_double_arrow", "ns-resize"],
            CursorStyle::ResizeUpLeftDownRight => &["size_fdiag", "bd_double_arrow", "nwse-resize"],
            CursorStyle::ResizeUpRightDownLeft => &["size_bdiag", "nesw-resize", "fd_double_arrow"],
            CursorStyle::ResizeColumn => &["col-resize", "sb_h_double_arrow"],
            CursorStyle::ResizeRow => &["row-resize", "sb_v_double_arrow"],
            CursorStyle::IBeamCursorForVerticalLayout => &["vertical-text"],
            CursorStyle::OperationNotAllowed => &["not-allowed", "crossed_circle"],
            CursorStyle::DragLink => &["alias"],
            CursorStyle::DragCopy => &["copy"],
            CursorStyle::ContextualMenu => &["context-menu"],
            CursorStyle::None => {
                #[cfg(debug_assertions)]
                panic!("CursorStyle::None should be handled separately in the client");
                #[cfg(not(debug_assertions))]
                &[DEFAULT_CURSOR_ICON_NAME]
            }
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
pub(super) fn log_cursor_icon_warning(message: impl std::fmt::Display) {
    if let Ok(xcursor_path) = env::var("XCURSOR_PATH") {
        log::warn!(
            "{:#}\ncursor icon loading may be failing if XCURSOR_PATH environment variable is invalid. \
                    XCURSOR_PATH overrides the default icon search. Its current value is '{}'",
            message,
            xcursor_path
        );
    } else {
        log::warn!("{:#}", message);
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
fn guess_ascii(keycode: Keycode, shift: bool) -> Option<char> {
    let c = match (keycode.raw(), shift) {
        (24, _) => 'q',
        (25, _) => 'w',
        (26, _) => 'e',
        (27, _) => 'r',
        (28, _) => 't',
        (29, _) => 'y',
        (30, _) => 'u',
        (31, _) => 'i',
        (32, _) => 'o',
        (33, _) => 'p',
        (34, false) => '[',
        (34, true) => '{',
        (35, false) => ']',
        (35, true) => '}',
        (38, _) => 'a',
        (39, _) => 's',
        (40, _) => 'd',
        (41, _) => 'f',
        (42, _) => 'g',
        (43, _) => 'h',
        (44, _) => 'j',
        (45, _) => 'k',
        (46, _) => 'l',
        (47, false) => ';',
        (47, true) => ':',
        (48, false) => '\'',
        (48, true) => '"',
        (49, false) => '`',
        (49, true) => '~',
        (51, false) => '\\',
        (51, true) => '|',
        (52, _) => 'z',
        (53, _) => 'x',
        (54, _) => 'c',
        (55, _) => 'v',
        (56, _) => 'b',
        (57, _) => 'n',
        (58, _) => 'm',
        (59, false) => ',',
        (59, true) => '>',
        (60, false) => '.',
        (60, true) => '<',
        (61, false) => '/',
        (61, true) => '?',

        _ => return None,
    };

    Some(c)
}

#[cfg(any(feature = "wayland", feature = "x11"))]
impl crate::Keystroke {
    pub(super) fn from_xkb(
        state: &State,
        mut modifiers: crate::Modifiers,
        keycode: Keycode,
    ) -> Self {
        let key_utf32 = state.key_get_utf32(keycode);
        let key_utf8 = state.key_get_utf8(keycode);
        let key_sym = state.key_get_one_sym(keycode);

        let key = match key_sym {
            Keysym::Return => "enter".to_owned(),
            Keysym::Prior => "pageup".to_owned(),
            Keysym::Next => "pagedown".to_owned(),
            Keysym::ISO_Left_Tab => "tab".to_owned(),
            Keysym::KP_Prior => "pageup".to_owned(),
            Keysym::KP_Next => "pagedown".to_owned(),
            Keysym::XF86_Back => "back".to_owned(),
            Keysym::XF86_Forward => "forward".to_owned(),
            Keysym::XF86_Cut => "cut".to_owned(),
            Keysym::XF86_Copy => "copy".to_owned(),
            Keysym::XF86_Paste => "paste".to_owned(),
            Keysym::XF86_New => "new".to_owned(),
            Keysym::XF86_Open => "open".to_owned(),
            Keysym::XF86_Save => "save".to_owned(),

            Keysym::comma => ",".to_owned(),
            Keysym::period => ".".to_owned(),
            Keysym::less => "<".to_owned(),
            Keysym::greater => ">".to_owned(),
            Keysym::slash => "/".to_owned(),
            Keysym::question => "?".to_owned(),

            Keysym::semicolon => ";".to_owned(),
            Keysym::colon => ":".to_owned(),
            Keysym::apostrophe => "'".to_owned(),
            Keysym::quotedbl => "\"".to_owned(),

            Keysym::bracketleft => "[".to_owned(),
            Keysym::braceleft => "{".to_owned(),
            Keysym::bracketright => "]".to_owned(),
            Keysym::braceright => "}".to_owned(),
            Keysym::backslash => "\\".to_owned(),
            Keysym::bar => "|".to_owned(),

            Keysym::grave => "`".to_owned(),
            Keysym::asciitilde => "~".to_owned(),
            Keysym::exclam => "!".to_owned(),
            Keysym::at => "@".to_owned(),
            Keysym::numbersign => "#".to_owned(),
            Keysym::dollar => "$".to_owned(),
            Keysym::percent => "%".to_owned(),
            Keysym::asciicircum => "^".to_owned(),
            Keysym::ampersand => "&".to_owned(),
            Keysym::asterisk => "*".to_owned(),
            Keysym::parenleft => "(".to_owned(),
            Keysym::parenright => ")".to_owned(),
            Keysym::minus => "-".to_owned(),
            Keysym::underscore => "_".to_owned(),
            Keysym::equal => "=".to_owned(),
            Keysym::plus => "+".to_owned(),
            Keysym::space => "space".to_owned(),
            Keysym::BackSpace => "backspace".to_owned(),
            Keysym::Tab => "tab".to_owned(),
            Keysym::Delete => "delete".to_owned(),
            Keysym::Escape => "escape".to_owned(),

            Keysym::Left => "left".to_owned(),
            Keysym::Right => "right".to_owned(),
            Keysym::Up => "up".to_owned(),
            Keysym::Down => "down".to_owned(),
            Keysym::Home => "home".to_owned(),
            Keysym::End => "end".to_owned(),
            Keysym::Insert => "insert".to_owned(),

            _ => {
                let name = xkb::keysym_get_name(key_sym).to_lowercase();
                if key_sym.is_keypad_key() {
                    name.replace("kp_", "")
                } else if let Some(key) = key_utf8.chars().next()
                    && key_utf8.len() == 1
                    && key.is_ascii()
                {
                    if key.is_ascii_graphic() {
                        key_utf8.to_lowercase()
                    // map ctrl-a to `a`
                    // ctrl-0..9 may emit control codes like ctrl-[, but
                    // we don't want to map them to `[`
                    } else if key_utf32 <= 0x1f
                        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
                    {
                        ((key_utf32 as u8 + 0x40) as char)
                            .to_ascii_lowercase()
                            .to_string()
                    } else {
                        name
                    }
                } else if let Some(key_en) = guess_ascii(keycode, modifiers.shift) {
                    String::from(key_en)
                } else {
                    name
                }
            }
        };

        if modifiers.shift {
            // we only include the shift for upper-case letters by convention,
            // so don't include for numbers and symbols, but do include for
            // tab/enter, etc.
            if key.chars().count() == 1 && key.to_lowercase() == key.to_uppercase() {
                modifiers.shift = false;
            }
        }

        // Ignore control characters (and DEL) for the purposes of key_char
        let key_char =
            (key_utf32 >= 32 && key_utf32 != 127 && !key_utf8.is_empty()).then_some(key_utf8);

        Self {
            modifiers,
            key,
            key_char,
        }
    }

    /**
     * Returns which symbol the dead key represents
     * <https://developer.mozilla.org/en-US/docs/Web/API/UI_Events/Keyboard_event_key_values#dead_keycodes_for_linux>
     */
    pub fn underlying_dead_key(keysym: Keysym) -> Option<String> {
        match keysym {
            Keysym::dead_grave => Some("`".to_owned()),
            Keysym::dead_acute => Some("´".to_owned()),
            Keysym::dead_circumflex => Some("^".to_owned()),
            Keysym::dead_tilde => Some("~".to_owned()),
            Keysym::dead_macron => Some("¯".to_owned()),
            Keysym::dead_breve => Some("˘".to_owned()),
            Keysym::dead_abovedot => Some("˙".to_owned()),
            Keysym::dead_diaeresis => Some("¨".to_owned()),
            Keysym::dead_abovering => Some("˚".to_owned()),
            Keysym::dead_doubleacute => Some("˝".to_owned()),
            Keysym::dead_caron => Some("ˇ".to_owned()),
            Keysym::dead_cedilla => Some("¸".to_owned()),
            Keysym::dead_ogonek => Some("˛".to_owned()),
            Keysym::dead_iota => Some("ͅ".to_owned()),
            Keysym::dead_voiced_sound => Some("゙".to_owned()),
            Keysym::dead_semivoiced_sound => Some("゚".to_owned()),
            Keysym::dead_belowdot => Some("̣̣".to_owned()),
            Keysym::dead_hook => Some("̡".to_owned()),
            Keysym::dead_horn => Some("̛".to_owned()),
            Keysym::dead_stroke => Some("̶̶".to_owned()),
            Keysym::dead_abovecomma => Some("̓̓".to_owned()),
            Keysym::dead_abovereversedcomma => Some("ʽ".to_owned()),
            Keysym::dead_doublegrave => Some("̏".to_owned()),
            Keysym::dead_belowring => Some("˳".to_owned()),
            Keysym::dead_belowmacron => Some("̱".to_owned()),
            Keysym::dead_belowcircumflex => Some("ꞈ".to_owned()),
            Keysym::dead_belowtilde => Some("̰".to_owned()),
            Keysym::dead_belowbreve => Some("̮".to_owned()),
            Keysym::dead_belowdiaeresis => Some("̤".to_owned()),
            Keysym::dead_invertedbreve => Some("̯".to_owned()),
            Keysym::dead_belowcomma => Some("̦".to_owned()),
            Keysym::dead_currency => None,
            Keysym::dead_lowline => None,
            Keysym::dead_aboveverticalline => None,
            Keysym::dead_belowverticalline => None,
            Keysym::dead_longsolidusoverlay => None,
            Keysym::dead_a => None,
            Keysym::dead_A => None,
            Keysym::dead_e => None,
            Keysym::dead_E => None,
            Keysym::dead_i => None,
            Keysym::dead_I => None,
            Keysym::dead_o => None,
            Keysym::dead_O => None,
            Keysym::dead_u => None,
            Keysym::dead_U => None,
            Keysym::dead_small_schwa => Some("ə".to_owned()),
            Keysym::dead_capital_schwa => Some("Ə".to_owned()),
            Keysym::dead_greek => None,
            _ => None,
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
impl crate::Modifiers {
    pub(super) fn from_xkb(keymap_state: &State) -> Self {
        let shift = keymap_state.mod_name_is_active(xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE);
        let alt = keymap_state.mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
        let control =
            keymap_state.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        let platform =
            keymap_state.mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);
        Self {
            shift,
            alt,
            control,
            platform,
            function: false,
        }
    }
}

#[cfg(any(feature = "wayland", feature = "x11"))]
impl crate::Capslock {
    pub(super) fn from_xkb(keymap_state: &State) -> Self {
        let on = keymap_state.mod_name_is_active(xkb::MOD_NAME_CAPS, xkb::STATE_MODS_EFFECTIVE);
        Self { on }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, px};
    #[cfg(any(feature = "wayland", feature = "x11"))]
    use std::io::Write as _;

    #[test]
    fn test_is_within_click_distance() {
        let zero = Point::new(px(0.0), px(0.0));
        assert!(is_within_click_distance(zero, Point::new(px(5.0), px(5.0))));
        assert!(is_within_click_distance(
            zero,
            Point::new(px(-4.9), px(5.0))
        ));
        assert!(is_within_click_distance(
            Point::new(px(3.0), px(2.0)),
            Point::new(px(-2.0), px(-2.0))
        ));
        assert!(!is_within_click_distance(
            zero,
            Point::new(px(5.0), px(5.1))
        ),);
    }

    #[cfg(any(feature = "wayland", feature = "x11"))]
    #[test]
    fn read_fd_bounded_reads_to_eof() {
        let mut pipe = filedescriptor::Pipe::new().unwrap();
        pipe.write.write_all(b"gnil").unwrap();
        drop(pipe.write);

        let bytes = read_fd_bounded(pipe.read, 4, Duration::from_secs(1)).unwrap();

        assert_eq!(bytes, b"gnil");
    }

    #[cfg(any(feature = "wayland", feature = "x11"))]
    #[test]
    fn read_fd_bounded_rejects_oversized_payload() {
        let mut pipe = filedescriptor::Pipe::new().unwrap();
        pipe.write.write_all(b"oversized").unwrap();
        drop(pipe.write);

        let error = read_fd_bounded(pipe.read, 4, Duration::from_secs(1)).unwrap_err();

        assert!(error.to_string().contains("exceeds 4 bytes"));
    }

    #[cfg(any(feature = "wayland", feature = "x11"))]
    #[test]
    fn read_fd_bounded_times_out_when_writer_stalls() {
        let pipe = filedescriptor::Pipe::new().unwrap();

        let error = read_fd_bounded(pipe.read, 4, Duration::from_millis(20)).unwrap_err();

        drop(pipe.write);
        assert!(error.to_string().contains("timed out"));
    }
}
