//! Lane D's platform checks: what a real desktop window must do beyond running catalog scenes.
//! The desktop host runs them under these ids, and `gpux-testkit` lists them.

/// One platform check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlatformCheck {
    /// Stable kebab-case id, used in result lines.
    pub id: &'static str,
    /// What it proves.
    pub description: &'static str,
}

/// Every platform check, in the order the desktop host runs them.
pub const PLATFORM_CHECKS: &[PlatformCheck] = &[
    PlatformCheck {
        id: "platform-open-present",
        description: "a window opens, presents a frame and renders it back through `render_to_image`",
    },
    PlatformCheck {
        id: "platform-repeated-capture",
        description: "`render_to_image` captures the same window repeatedly without starving its surface",
    },
    PlatformCheck {
        id: "platform-resize",
        description: "resizing the window changes its viewport and the rendered image size",
    },
    PlatformCheck {
        id: "platform-scale-change",
        description: "a scale-factor change from the platform re-renders at the new scale",
    },
    PlatformCheck {
        id: "platform-clipboard",
        description: "text written to the platform clipboard reads back unchanged",
    },
    PlatformCheck {
        id: "platform-os-input",
        description: "OS-level synthetic mouse and keyboard input reaches GPUI as the expected events",
    },
    PlatformCheck {
        id: "platform-close",
        description: "closing a window removes it without panics",
    },
];

/// The platform check with this id.
pub fn platform_check(id: &str) -> Option<&'static PlatformCheck> {
    PLATFORM_CHECKS.iter().find(|check| check.id == id)
}
