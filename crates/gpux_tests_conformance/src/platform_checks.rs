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
        id: "platform-clipboard-html",
        description: "text written to the platform clipboard with HTML beside it reads back as the same text with that HTML",
    },
    PlatformCheck {
        id: "platform-os-input",
        description: "OS-level synthetic mouse and keyboard input reaches GPUI as the expected events",
    },
    PlatformCheck {
        id: "platform-close",
        description: "closing a window removes it and releases its views, without panics",
    },
    PlatformCheck {
        id: "devtools-hello-snapshot",
        description: "a real window connected to a gpux-devtools sidecar says `hello` naming the app and its window, and a snapshot of it shows its button",
    },
    PlatformCheck {
        id: "devtools-tap",
        description: "a click the sidecar synthesizes at a button's ref reaches the button, and the window's tree shows its effect",
    },
    PlatformCheck {
        id: "screen-reader-macos-voiceover",
        description: "on macOS, VoiceOver's view of the `accessibility` scene (the window view's `accessibilityChildren`) gives the table its rows and cell positions, the disclosure button its expanded state, the spin button its value, the disabled button a press action and the slider a horizontal orientation and a one-step increment (skipped elsewhere)",
    },
    PlatformCheck {
        id: "composition-macos-order-bounds",
        description: "with window composition on macOS, a native surface's view sits between the GPUI base view and the overlay view, at its element's bounds in device pixels",
    },
    PlatformCheck {
        id: "composition-macos-input",
        description: "with window composition on macOS, a click passes through the empty overlay to the native view, and is taken by the overlay while a GPUI menu covers it",
    },
    PlatformCheck {
        id: "composition-macos-shadow",
        description: "with window composition on macOS, an OS screenshot shows a GPUI menu's shadow blended over a solid-colour native view (skipped without screen-capture permission)",
    },
    PlatformCheck {
        id: "composition-wayland-order",
        description: "with window composition on Wayland, native surfaces are subsurfaces stacked in composition order below the overlay, nested after a reparent, and the compositor accepts every placement (skipped outside a Wayland session)",
    },
    PlatformCheck {
        id: "composition-wayland-geometry",
        description: "with window composition on Wayland, a native subsurface is positioned and sized at its element's bounds in logical pixels and follows it (skipped outside a Wayland session)",
    },
    PlatformCheck {
        id: "composition-wayland-input",
        description: "with window composition on Wayland, the overlay's input region is empty without content and is the open menu's rectangle, not its shadow (skipped outside a Wayland session)",
    },
    PlatformCheck {
        id: "composition-x11-order-bounds",
        description: "with window composition on X11, native surfaces are mapped child windows stacked in composition order at their elements' bounds in device pixels (skipped outside X11)",
    },
    PlatformCheck {
        id: "composition-x11-cutouts",
        description: "with window composition on X11, an open GPUI menu is cut out of the native window's bounding shape while its shadow is not (skipped outside X11)",
    },
    PlatformCheck {
        id: "composition-x11-input",
        description: "with window composition on X11, the server's hit testing finds the native window over its area and the GPUI window under an open menu (skipped outside X11)",
    },
    PlatformCheck {
        id: "composition-x11-overlay",
        description: "with window composition on X11 under a compositing manager, the GPUI content above native surfaces is drawn into an override-redirect overlay window whose input shape is that content (skipped without a compositing manager)",
    },
    PlatformCheck {
        id: "composition-windows-order",
        description: "with window composition on Windows, an OS screenshot shows a native surface's DirectComposition visual above the base content and below a GPUI menu (skipped outside Windows)",
    },
    PlatformCheck {
        id: "composition-windows-clip",
        description: "with window composition on Windows, a native surface's content presented larger than its bounds is clipped to them (skipped outside Windows)",
    },
    PlatformCheck {
        id: "composition-windows-device-loss",
        description: "with window composition on Windows, the device-lost message rebuilds the DirectComposition tree, tells the native surface's owner once, and its content shows again (skipped outside Windows)",
    },
    PlatformCheck {
        id: "composition-windows-accelerator",
        description: "with window composition on Windows, a key down for a non-GPUI child window reaches its window procedure translated (skipped outside Windows)",
    },
    PlatformCheck {
        id: "composition-windows-resize",
        description: "with window composition on Windows, fast height-only live resize leaves the base content unstretched, and reports the visual-hosted alternative's behaviour (skipped outside Windows)",
    },
];

/// The platform check with this id.
pub fn platform_check(id: &str) -> Option<&'static PlatformCheck> {
    PLATFORM_CHECKS.iter().find(|check| check.id == id)
}

/// The clipboard-with-HTML check every lane's host runs (`platform-clipboard-html` on the
/// desktop, `clipboard-android` and `clipboard-ios` on mobile): plain text with HTML beside it.
pub mod clipboard_html {
    use gpux::ClipboardItem;

    /// The item to write: `nonce` makes it distinguishable from an earlier run's.
    pub fn item(nonce: u32) -> ClipboardItem {
        ClipboardItem::new_string_with_html(
            format!("gpux ✓ html {nonce}"),
            format!("<b>gpux ✓ html</b> {nonce}"),
        )
    }

    /// Whether `read` carries `written`'s text exactly and its HTML, which a platform may wrap
    /// (Windows' `HTML Format` header, a pasteboard's document wrapper) but never change: `Ok`
    /// with a summary, or `Err` with what differs.
    pub fn verify(written: &ClipboardItem, read: Option<&ClipboardItem>) -> Result<String, String> {
        let Some(read) = read else {
            return Err("the clipboard read back nothing".into());
        };
        let text = read.text();
        if text != written.text() {
            return Err(format!(
                "wrote text {:?} and read back {text:?}",
                written.text()
            ));
        }
        let expected = written.html().unwrap_or_default();
        match read.html() {
            Some(html) if html.contains(expected) => Ok(format!(
                "{} text characters, {} HTML bytes",
                text.unwrap_or_default().chars().count(),
                html.len()
            )),
            html => Err(format!("wrote HTML {expected:?} and read back {html:?}")),
        }
    }
}
