//! The operating system whose keyboard conventions GPUI follows: which modifier is the
//! "secondary" one, how keystrokes are written and displayed, and the `os` key context. Native
//! targets fix it at compile time. In a browser the compile target says nothing about the
//! visitor's keyboard, so the web platform reports the browser's operating system at startup.

/// An operating system's keyboard conventions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyboardOs {
    /// macOS, and iOS or iPadOS in a browser: Command is the secondary modifier.
    MacOs,
    /// Linux and FreeBSD: Control is the secondary modifier.
    Linux,
    /// Windows: Control is the secondary modifier.
    Windows,
    /// Any other system: Control is the secondary modifier.
    Unknown,
}

impl KeyboardOs {
    /// The conventions of the compile target.
    #[cfg(not(target_family = "wasm"))]
    const NATIVE: Self = if cfg!(target_os = "macos") {
        Self::MacOs
    } else if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        Self::Linux
    } else if cfg!(target_os = "windows") {
        Self::Windows
    } else {
        Self::Unknown
    };

    /// The conventions in effect: the compile target's on native platforms; in a browser, those of
    /// the operating system the web platform reported, and [`KeyboardOs::Unknown`] until then.
    pub fn current() -> Self {
        #[cfg(target_family = "wasm")]
        {
            web::get()
        }
        #[cfg(not(target_family = "wasm"))]
        {
            Self::NATIVE
        }
    }

    /// Reports the browser's operating system. The web platform calls it when it starts, before
    /// any keystroke is parsed; key bindings parsed earlier keep the conventions they were parsed
    /// with.
    #[cfg(target_family = "wasm")]
    pub fn set_web(os: Self) {
        web::set(os);
    }

    /// Whether Command, rather than Control, is the secondary modifier.
    pub fn is_mac(self) -> bool {
        self == Self::MacOs
    }

    /// The value of the `os` key in [`crate::KeyContext::new_with_defaults`].
    pub fn key_context_name(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(target_family = "wasm")]
mod web {
    use super::KeyboardOs;
    use std::sync::atomic::{AtomicU8, Ordering};

    static OS: AtomicU8 = AtomicU8::new(KeyboardOs::Unknown as u8);

    pub(super) fn get() -> KeyboardOs {
        match OS.load(Ordering::Relaxed) {
            value if value == KeyboardOs::MacOs as u8 => KeyboardOs::MacOs,
            value if value == KeyboardOs::Linux as u8 => KeyboardOs::Linux,
            value if value == KeyboardOs::Windows as u8 => KeyboardOs::Windows,
            _ => KeyboardOs::Unknown,
        }
    }

    pub(super) fn set(os: KeyboardOs) {
        OS.store(os as u8, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod keyboard_os_tests {
    use super::KeyboardOs;
    use crate::platform::keystroke::{display_keystroke, parse_keystroke, unparse_keystroke};
    use crate::{KeyContext, Keystroke, Modifiers};

    const ALL: [KeyboardOs; 4] = [
        KeyboardOs::MacOs,
        KeyboardOs::Linux,
        KeyboardOs::Windows,
        KeyboardOs::Unknown,
    ];

    #[test]
    fn native_targets_keep_their_compile_time_conventions() {
        let expected = if cfg!(target_os = "macos") {
            KeyboardOs::MacOs
        } else if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            KeyboardOs::Linux
        } else if cfg!(target_os = "windows") {
            KeyboardOs::Windows
        } else {
            KeyboardOs::Unknown
        };
        assert_eq!(KeyboardOs::current(), expected);
        assert_eq!(
            KeyContext::new_with_defaults()
                .get("os")
                .map(|os| os.as_ref()),
            Some(expected.key_context_name())
        );
    }

    #[test]
    fn command_is_secondary_on_macos_and_control_elsewhere() {
        let command = Modifiers::command();
        let control = Modifiers::control();
        for os in ALL {
            let (secondary, other) = if os.is_mac() {
                (command, control)
            } else {
                (control, command)
            };
            assert_eq!(Modifiers::secondary_key_for(os), secondary, "{os:?}");
            assert!(secondary.secondary_for(os), "{os:?}");
            assert!(!other.secondary_for(os), "{os:?}");
            let parsed = parse_keystroke("secondary-a", os).unwrap();
            assert_eq!(parsed.modifiers, secondary, "{os:?}");
            assert_eq!(parsed.key, "a");
        }
    }

    #[test]
    fn keystrokes_with_the_platform_key_round_trip() {
        for os in ALL {
            for source in ["cmd-a", "ctrl-alt-shift-cmd-left", "cmd-shift-z"] {
                let keystroke = Keystroke::parse(source).unwrap();
                let written = unparse_keystroke(&keystroke.modifiers, &keystroke.key, os);
                assert_eq!(
                    parse_keystroke(&written, os).unwrap(),
                    keystroke,
                    "{os:?} wrote {source} as {written}"
                );
            }
        }
        let cmd_a = Keystroke::parse("cmd-a").unwrap();
        let written = |os| unparse_keystroke(&cmd_a.modifiers, &cmd_a.key, os);
        assert_eq!(written(KeyboardOs::MacOs), "cmd-a");
        assert_eq!(written(KeyboardOs::Linux), "super-a");
        assert_eq!(written(KeyboardOs::Windows), "win-a");
        assert_eq!(written(KeyboardOs::Unknown), "super-a");
    }

    #[test]
    fn keystrokes_display_in_each_systems_style() {
        let keystroke = Keystroke::parse("ctrl-alt-shift-cmd-left").unwrap();
        let shown = |os| display_keystroke(&keystroke.modifiers, &keystroke.key, os);
        assert_eq!(shown(KeyboardOs::MacOs), "^⌥⌘⇧←");
        assert_eq!(shown(KeyboardOs::Linux), "ctrl-alt-❖shift-left");
        assert_eq!(shown(KeyboardOs::Windows), "ctrl-alt-⊞shift-left");
        assert_eq!(shown(KeyboardOs::Unknown), "ctrl-alt-super-shift-left");
    }

    #[test]
    fn key_contexts_name_each_system() {
        let names: Vec<_> = ALL.iter().map(|os| os.key_context_name()).collect();
        assert_eq!(names, ["macos", "linux", "windows", "unknown"]);
        let context = KeyContext::with_defaults_for(KeyboardOs::Windows);
        assert_eq!(context.get("os").map(|os| os.as_ref()), Some("windows"));
    }
}
