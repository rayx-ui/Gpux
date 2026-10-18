# gpux

`gpux` is the RayX-maintained fork of [GPUI](https://gpui.rs), the GPU-accelerated UI framework
from the [Zed](https://github.com/zed-industries/zed) editor. It keeps Zed's GPUI crate closure and
crate boundaries and carries RayX's patch stack on top: web (WebGPU/WebGL) and mobile (Android,
iOS) platforms, a wgpu renderer on Windows, Linux and the web, frame and shader effects,
fixed/sticky positioning, programmatic scrolling, editable text, and an AccessKit accessibility
tree that screen readers read on every platform and agents inspect through gpux-devtools.

For GPUI concepts, [gpui.rs](https://gpui.rs) remains a useful introduction; for exact APIs and
behavior, use this repository, because gpux differs from upstream GPUI.

## Usage

gpux is consumed by release tag; take the latest
[release](https://github.com/rayx-ui/Gpux/releases):

```toml
[dependencies]
gpux = { git = "https://github.com/rayx-ui/Gpux", tag = "v1.0.0" }
gpui_platform = { package = "gpux-platform", git = "https://github.com/rayx-ui/Gpux", tag = "v1.0.0" }
```

```rust
use gpui_platform::application;
use gpux::{Context, Window, WindowOptions, div, prelude::*};

struct Hello;

impl Render for Hello {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Hello from gpux")
    }
}

fn main() {
    application().run(|cx| async move {
        cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Hello))
                .expect("open the window");
        });
    });
}
```

`gpux-platform` selects the platform crate for the compilation target. Packages are named
`gpux-*` and keep their GPUI Rust names, so `gpux-platform` is imported as `gpui_platform` (its
dependency key). `Cargo.lock` records the tag's commit, and tags never move. Nothing is published
to a registry. Building apps with rayx needs rayx 1.4.0 or later, the release that reads the
declarations gpux 1.0.0 makes (its font assets, Android Java sources and Android pins).
[docs/installation-and-usage.md](docs/installation-and-usage.md) has the features and
dev-dependencies, and `crates/gpui/examples` has more examples, including the web entry point
(`gpui_platform::web_init`).

## Crates

| Directory | Package | Rust name | Purpose |
|---|---|---|---|
| `crates/gpui` | `gpux` | `gpux` | The framework: elements, layout, text, input, windows, executors |
| `crates/gpui_platform` | `gpux-platform` | `gpui_platform` | Platform selection for the current target |
| `crates/gpui_windows` | `gpux-windows` | `gpui_windows` | Windows platform (DirectX 12 through wgpu) |
| `crates/gpui_macos` | `gpux-macos` | `gpui_macos` | macOS platform (Metal) |
| `crates/gpui_linux` | `gpux-linux` | `gpui_linux` | Linux platform (Wayland and X11) |
| `crates/gpui_web` | `gpux-web` | `gpui_web` | Browser platform (WebGPU, WebGL fallback) |
| `crates/gpui_mobile` | `gpux-mobile` | `gpui_mobile` | Android and iOS platforms |
| `crates/gpui_apple` | `gpux-apple` | `gpui_apple` | Metal renderer and dispatcher shared by the Apple platforms |
| `crates/gpui_wgpu` | `gpux-wgpu-renderer` | `gpui_wgpu` | wgpu renderer and cosmic-text text system |
| `crates/gpui_macros` | `gpux-macros` | `gpui_macros` | Derive and attribute macros |
| `crates/gpux_color` | `gpux-color` | `gpux_color` | Standards color engine behind `gpux::Color` |
| `crates/gpux_fonts` | `gpux-fonts` | `gpux_fonts` | Bundled IBM Plex Sans and Lilex fonts |
| `crates/gpux_devtools` | `gpux-devtools` | `gpux_devtools` | Agent inspection: connects an app to the `gpux-devtools-mcp` sidecar |
| `crates/gpux_devtools_mcp` | `gpux-devtools-mcp` | `gpux_devtools_mcp` | The stdio MCP sidecar agents use to inspect and drive gpux apps |
| `crates/gpux_tests_conformance` | `gpux-tests-conformance` | `gpux_tests_conformance` | Conformance scenes, test hosts and image comparison |
| `crates/gpui_shared_string` | `gpux-shared-string` | `gpui_shared_string` | `SharedString` |
| `crates/gpui_util` | `gpux-util` | `gpui_util` | GPUI utilities |
| `crates/collections` | `gpux-collections` | `collections` | Hash maps and sets |
| `crates/scheduler` | `gpux-scheduler` | `scheduler` | Task scheduling for executors |
| `crates/sum_tree` | `gpux-sum-tree` | `sum_tree` | B+ tree with summaries |
| `crates/refineable` | `gpux-refineable` | `refineable` | Style refinement |
| `crates/refineable/derive_refineable` | `gpux-derive-refineable` | `derive_refineable` | `#[derive(Refineable)]` |
| `crates/http_client` | `gpux-http-client` | `http_client` | HTTP client abstraction |
| `crates/http_client_tls` | `gpux-http-client-tls` | `http_client_tls` | TLS configuration |
| `crates/reqwest_client` | `gpux-reqwest-client` | `reqwest_client` | reqwest-backed HTTP client |
| `crates/path` | `gpux-path` | `path` | Path helpers |
| `crates/util` | `gpux-zed-util` | `util` | Zed's general utilities |
| `crates/util_macros` | `gpux-zed-util-macros` | `util_macros` | Zed's utility macros |
| `crates/bench_metrics` | `gpux-bench-metrics` | `bench_metrics` | Multi-metric Criterion measurements for the benchmarks |
| `tools/perf` | `gpux-perf` | `perf` | Test performance measurement |
| `tools/testkit` | `gpux-testkit` | `gpux_testkit` | The `gpux-testkit` test-lane runner |
| `vendor/accesskit_macos` | `gpux-accesskit-macos` | `accesskit_macos` | Temporary AccessKit macOS adapter fork, see below |
| `vendor/wgpu/...` | `gpux-wgpu`, `gpux-wgpu-core`, `gpux-wgpu-hal`, `gpux-wgpu-core-deps-*` | `wgpu`, ... | Temporary wgpu fork, see below |

Crates prefixed `gpux_tests_` verify gpux; no consumer depends on them:
`crates/gpux_tests_conformance` (above) and
[`crates/gpux_tests_benchmark`](crates/gpux_tests_benchmark/README.md), the improvement-loop
benchmarks, a standalone workspace that is never published. [`tools/`](tools/README.md) holds the
test runner, the `#[perf]` timer, the Playwright project and the mobile app shells.

## Fast mode

gpux carries the incremental-rendering work of [gpui-fast](docs/upstream/gpui-fast.md): layout
retention, view retention, splice, scroll layers (scrolled content drawn once into GPU tiles) and
opt-in native window composition. The first four are on by default; `GPUX_FAST_MODE=0` turns them
off. [docs/fast-mode.md](docs/fast-mode.md) describes the switches, what each mechanism costs and
saves, and how it is measured.

## Rich editable text

`EditableText` is a plain text field that becomes a rich text editor once its owner stores
formatting: per-character attributes (font, size, letter spacing, baseline shift, colors,
decorations), rows of different heights, paragraph formats, lists, inline objects, and HTML
beside plain text on every platform's clipboard. [docs/editable-text.md](docs/editable-text.md)
describes the API.

## The wgpu fork

`vendor/wgpu` contains `gpux-wgpu`, upstream wgpu 30.0.1's `wgpu`, `wgpu-core`, `wgpu-hal` and
`wgpu-core-deps-*` crates with a single change: DirectX 12 swap chains created for a
window handle (HWND) use `DXGI_SCALING_NONE` instead of `DXGI_SCALING_STRETCH`, so resizing a
Windows window does not stretch the previous frame
([gfx-rs/wgpu#10196](https://github.com/gfx-rs/wgpu/issues/10196)). The fork exists only until
upstream wgpu ships an equivalent option; gpux then returns to the crates.io `wgpu` release.
[docs/upstream/wgpu-dx12-swapchain-scaling.md](docs/upstream/wgpu-dx12-swapchain-scaling.md)
tracks the upstream change.

## The accesskit_macos fork

`vendor/accesskit_macos` contains `gpux-accesskit-macos`, upstream `accesskit_macos` 0.27.1 with
the changes VoiceOver needs to read a table's rows and cells, an expanded state, a range
widget's value (and to adjust it by increment) and a disabled control as dimmed. The fork exists
only until an AccessKit release ships them;
[docs/upstream/accesskit-macos-tables-expanded.md](docs/upstream/accesskit-macos-tables-expanded.md)
lists the changes and tracks the upstream change.

## Maintenance

Upstream imports (Zed GPUI closure, gpui-mobile, wgpu), installation, platform prerequisites and
validation are documented in the [gpux skill](.claude/skills/gpux/SKILL.md). gpux tests itself in
lanes run by `gpux-testkit`, from the test platform to browsers, desktop windows, Android, iOS
and the local GPU; [docs/testing.md](docs/testing.md) describes them. Releases (one `vX.Y.Z` tag
for the whole workspace, cut by release-plz from Conventional Commits) are described in
[docs/releasing.md](docs/releasing.md). Contributor rules are in [CLAUDE.md](CLAUDE.md).

## License

gpux is licensed under the [Apache License, Version 2.0](LICENSE-APACHE), like upstream GPUI. The
bundled fonts in `crates/gpux_fonts` are licensed under the SIL Open Font License 1.1.

## Acknowledgements

[Zed Industries](https://github.com/zed-industries/zed) (GPUI) ·
[gpui-mobile](https://github.com/itsbalamurali/gpui-mobile) · [wgpu](https://wgpu.rs/) ·
[cosmic-text](https://github.com/pop-os/cosmic-text)
