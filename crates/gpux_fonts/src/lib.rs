//! The fonts gpux bundles: IBM Plex Sans for interface text and Lilex for code. Both families are
//! licensed under the SIL Open Font License 1.1; the license texts sit beside the files in
//! `assets/fonts/`.
//!
//! `assets/` is also an asset root: it holds the files at the `fonts/...` paths gpux's SVG
//! renderer loads through an application's `AssetSource`.

pub const IBM_PLEX_SANS_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf");
pub const IBM_PLEX_SANS_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-Italic.ttf");
pub const IBM_PLEX_SANS_SEMIBOLD: &[u8] =
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf");
pub const IBM_PLEX_SANS_SEMIBOLD_ITALIC: &[u8] =
    include_bytes!("../assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBoldItalic.ttf");
pub const LILEX_REGULAR: &[u8] = include_bytes!("../assets/fonts/lilex/Lilex-Regular.ttf");
pub const LILEX_BOLD: &[u8] = include_bytes!("../assets/fonts/lilex/Lilex-Bold.ttf");
pub const LILEX_ITALIC: &[u8] = include_bytes!("../assets/fonts/lilex/Lilex-Italic.ttf");
pub const LILEX_BOLD_ITALIC: &[u8] = include_bytes!("../assets/fonts/lilex/Lilex-BoldItalic.ttf");

/// Every bundled font file.
pub const ALL: [&[u8]; 8] = [
    IBM_PLEX_SANS_REGULAR,
    IBM_PLEX_SANS_ITALIC,
    IBM_PLEX_SANS_SEMIBOLD,
    IBM_PLEX_SANS_SEMIBOLD_ITALIC,
    LILEX_REGULAR,
    LILEX_BOLD,
    LILEX_ITALIC,
    LILEX_BOLD_ITALIC,
];
