//! The visitor's operating system, whose keyboard conventions GPUI follows in the browser: Command
//! as the secondary modifier on Apple systems, Control elsewhere.

use gpux::KeyboardOs;

/// The keyboard conventions of a browser that reports `platform` (`navigator.platform`) and
/// `user_agent` (`navigator.userAgent`). `navigator.platform` is deprecated but still the most
/// widely available answer; the user agent decides when it is missing or names no known system.
pub(crate) fn keyboard_os(platform: Option<&str>, user_agent: Option<&str>) -> KeyboardOs {
    let user_agent = user_agent.unwrap_or_default();
    // Android reports a Linux platform ("Linux armv8l") but follows no desktop convention.
    if user_agent.contains("Android") {
        return KeyboardOs::Unknown;
    }
    let from_platform = platform.and_then(|platform| {
        if ["Mac", "iPhone", "iPad", "iPod"]
            .iter()
            .any(|name| platform.contains(name))
        {
            Some(KeyboardOs::MacOs)
        } else if platform.starts_with("Win") {
            Some(KeyboardOs::Windows)
        } else if ["Linux", "X11", "FreeBSD"]
            .iter()
            .any(|name| platform.contains(name))
        {
            Some(KeyboardOs::Linux)
        } else {
            None
        }
    });
    from_platform.unwrap_or_else(|| {
        // iPhone and iPad user agents also say "like Mac OS X".
        if user_agent.contains("Mac") {
            KeyboardOs::MacOs
        } else if user_agent.contains("Windows") {
            KeyboardOs::Windows
        } else if ["Linux", "X11", "CrOS", "FreeBSD"]
            .iter()
            .any(|name| user_agent.contains(name))
        {
            KeyboardOs::Linux
        } else {
            KeyboardOs::Unknown
        }
    })
}

/// The keyboard conventions of the browser `browser_window` runs in.
#[cfg(target_family = "wasm")]
pub(crate) fn keyboard_os_of(browser_window: &web_sys::Window) -> KeyboardOs {
    let navigator = browser_window.navigator();
    #[allow(deprecated)]
    let platform = navigator.platform().ok();
    let user_agent = navigator.user_agent().ok();
    keyboard_os(platform.as_deref(), user_agent.as_deref())
}

#[cfg(test)]
mod browser_os_tests {
    use super::keyboard_os;
    use gpux::KeyboardOs;

    const MAC_CHROME: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                              (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
    const IPHONE_SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) \
                                 AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 \
                                 Mobile/15E148 Safari/604.1";
    const WINDOWS_EDGE: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                                (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0";
    const LINUX_FIREFOX: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 \
                                 Firefox/140.0";
    const CHROME_OS: &str = "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 \
                             (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
    const ANDROID_CHROME: &str = "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 \
                                  (KHTML, like Gecko) Chrome/147.0.0.0 Mobile Safari/537.36";

    #[test]
    fn apple_browsers_follow_macos_conventions() {
        assert_eq!(
            keyboard_os(Some("MacIntel"), Some(MAC_CHROME)),
            KeyboardOs::MacOs
        );
        assert_eq!(
            keyboard_os(Some("iPhone"), Some(IPHONE_SAFARI)),
            KeyboardOs::MacOs
        );
        assert_eq!(keyboard_os(Some("iPad"), None), KeyboardOs::MacOs);
        assert_eq!(keyboard_os(None, Some(MAC_CHROME)), KeyboardOs::MacOs);
    }

    #[test]
    fn windows_and_linux_browsers_follow_their_systems() {
        assert_eq!(
            keyboard_os(Some("Win32"), Some(WINDOWS_EDGE)),
            KeyboardOs::Windows
        );
        assert_eq!(
            keyboard_os(Some(""), Some(WINDOWS_EDGE)),
            KeyboardOs::Windows
        );
        assert_eq!(
            keyboard_os(Some("Linux x86_64"), Some(LINUX_FIREFOX)),
            KeyboardOs::Linux
        );
        assert_eq!(keyboard_os(None, Some(CHROME_OS)), KeyboardOs::Linux);
    }

    #[test]
    fn android_and_unknown_browsers_use_control() {
        assert_eq!(
            keyboard_os(Some("Linux armv8l"), Some(ANDROID_CHROME)),
            KeyboardOs::Unknown
        );
        assert_eq!(keyboard_os(None, None), KeyboardOs::Unknown);
        assert_eq!(keyboard_os(Some("Nintendo"), Some("")), KeyboardOs::Unknown);
        assert!(!keyboard_os(None, None).is_mac());
    }
}
