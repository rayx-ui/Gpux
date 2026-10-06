use gpux::App;

/// Loads the fonts the examples render with.
///
/// gpux platforms ship the bundled IBM Plex Sans and Lilex fonts, including the
/// browser platform, so the examples have nothing extra to load.
pub fn load_fonts(_cx: &App) -> bool {
    true
}
