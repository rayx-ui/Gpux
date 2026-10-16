# Upstream: AccessKit 0.25 ahead of Zed

Zed pins the AccessKit 0.24 family (`accesskit` 0.24, `accesskit_macos` 0.26, `accesskit_windows`
0.34, `accesskit_unix` 0.22). gpux moved to the 0.25 family on 2026-10-16
(gpux-a11y-devtools epic), ahead of Zed, because 0.25 is the family the mobile adapters
`accesskit_ios` 0.2 and `accesskit_android` 0.9 build on.

| Crate | gpux pin | Zed pin at `d544470d` |
|---|---|---|
| `accesskit` (with `enumn`) | 0.25.1 | 0.24 |
| `accesskit_consumer` | 0.39.1 | (through the adapters) 0.38 |
| `accesskit_macos` | 0.27.1 | 0.26 |
| `accesskit_windows` | 0.35.1 | 0.34 |
| `accesskit_unix` | 0.24.0 | 0.22 |
| `accesskit_ios` | 0.2.1 (gpux-only) | none |
| `accesskit_android` | 0.9.0 (gpux-only) | none |

The move's only breaking change for gpux was `accesskit::Tree` becoming `accesskit::TreeInfo`
(the old name is a deprecated alias). gpux builds its `TreeInfo` in one place,
`window::a11y::tree_info()`, which also names gpux as the toolkit (`toolkit_name` "gpux",
`toolkit_version` the crate version). `crates/gpui/tests/accesskit_025.rs` pins the 0.25
behaviours gpux relies on (cloned nodes keep their properties, a cleared property can be set
again, node equality ignores the order properties were set in).

## On a Zed import

- Keep gpux's pins in the workspace `Cargo.toml` unless Zed's are newer; never take Zed's older
  pins with its manifest.
- Imported code that writes `accesskit::Tree::new(...)` becomes `tree_info()` (or `TreeInfo`).
- All adapters move together: `Cargo.lock` must hold one `accesskit` version.

When Zed reaches the 0.25 family or later, this page and the gpux-local delta it records go away.
