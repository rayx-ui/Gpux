# Installation and Usage

Use this reference for gpux dependency setup, examples, local validation, and deciding which platform prerequisite reference to load. It is the agent workflow; the human-facing guide is [docs/installation-and-usage.md](../../../../docs/installation-and-usage.md), so keep setup facts in both consistent.

Run commands from the Gpux repository root. RayX validation steps run from `../RayX`.

## Workflow

1. Identify the user's goal: consume `gpux` from another Rust project, build this repository, run examples, install prerequisites, configure WASM/browser support, or fix a build failure.
2. Inspect the host platform before recommending or running installers:
   - Windows: `$env:PROCESSOR_ARCHITECTURE`, `winget --version`, `rustc --version`, `cargo --version`, `rustup toolchain list`, `rustup target list --installed`, `fxc /help`, `clang --version`, `wasm-bindgen --version`, Android SDK/NDK environment variables, `cargo ndk --version`.
   - macOS/Linux: `uname -a`, package manager availability, `rustc --version`, `cargo --version`, `rustup toolchain list`, `rustup target list --installed`, `clang --version`, `wasm-bindgen --version`, `xcodebuild -version`, `xcodegen --version`, Android SDK/NDK environment variables, `cargo ndk --version`.
3. Load the platform reference that matches the request:
   - Windows or Windows ARM64: `references/windows-prerequisites.md`.
   - macOS, Linux, or browser/WASM: `references/macos-linux-wasm-prerequisites.md`.
   - Multithreaded gpux/RayX browser apps: `references/multithreaded-wasm-apps.md`.
4. If the user explicitly asks you to install prerequisites, run check commands first, then run the smallest install command or helper script needed.
5. Validate with the relevant Cargo checks after setup, from the Gpux repository root.

## Cargo Dependency Setup

gpux publishes to crates.io. The repository keeps the upstream closure path `crates/gpui`, but the public Rust package and import crate are named `gpux`; the other crates publish as `gpux-*` packages that keep their GPUI Rust names (the [README crate table](../../../../README.md#crates) lists them).

From crates.io (`gpux-platform` is imported as `gpui_platform`):

```toml
[dependencies]
gpux = "0.3"
gpux-platform = "0.3"

[dev-dependencies]
gpux = { version = "0.3", features = ["test-support"] }
gpux-platform = { version = "0.3", features = ["test-support"] }
```

From a sibling checkout of this repository, for co-development (RayX uses this form):

```toml
[dependencies]
gpux = { path = "../Gpux/crates/gpui" }
gpui_platform = { package = "gpux-platform", path = "../Gpux/crates/gpui_platform" }

[dev-dependencies]
gpux = { path = "../Gpux/crates/gpui", features = ["test-support"] }
gpui_platform = { package = "gpux-platform", path = "../Gpux/crates/gpui_platform", features = ["test-support"] }
```

Enable features as needed, prefixed with the dependency key (`gpux-platform/...` in the crates.io form, `gpui_platform/...` in the sibling form):

- `gpui_platform/font-kit` for macOS font-kit support.
- `gpui_platform/wayland` and `gpui_platform/x11` for Linux windowing backends.
- `gpux/test-support` and `gpui_platform/test-support` for GPUI tests.
- `gpux/inspector`, `gpux/screen-capture`, `gpux/leak-detection`, and `gpux/input-latency-histogram` for targeted diagnostics.

Always import from `gpux::{...}` in user code.

## Minimal Native App

```rust
use gpux::{div, prelude::*, App, Context, IntoElement, Window, WindowOptions};
use gpui_platform::application;

struct Hello;

impl Render for Hello {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().child("Hello from gpux")
    }
}

fn main() {
    application().run(|app| async move {
        app.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Hello))
                .unwrap();
        });
    });
}
```

## Examples and Validation

Run examples from the Gpux repository root:

```bash
cargo run -p gpux --example hello_world
cargo run -p gpux --example input
cargo run -p gpux --example uniform_list
```

Use these baseline checks for local development:

```bash
cargo metadata --no-deps
cargo fmt --check
cargo check -p gpux --all-targets
cargo test -p gpux --features test-support
```

For platform, renderer, shader, or support-crate work, check touched packages directly by package name:

```bash
cargo check -p gpux-platform --all-targets
cargo check -p gpux-wgpu-renderer --all-targets
cargo check -p gpux-windows --all-targets
cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features
cargo ndk -t arm64-v8a check -p gpux-mobile
```

Some package checks depend on host platform SDKs. The web check disables default features because the default multithreaded build needs nightly `-Z build-std`, which RayX's xtask provides. The Android check needs `cargo-ndk` and `ANDROID_NDK_HOME`. Run `cargo package --workspace` after manifest or dependency changes.

## Baseline Toolchain Prerequisites

Rule: Check Rust, browser/WASM tools, and mobile platform tools before debugging gpux or RayX mobile validation failures.

Required baseline:

- Rust 1.95 or newer (the gpux workspace `rust-version`; `rust-toolchain.toml` pins 1.95.0, like RayX).
- Nightly Rust plus `rust-src` for multithreaded WASM builds that use `wasm_thread`.
- `wasm-bindgen-cli 0.2.126` for the current RayX browser build.
- iOS/macOS validation: macOS with Xcode 15 or newer and XcodeGen.
- Android validation: Android SDK, Android NDK r25 or newer, Java/JDK discovery, and `cargo-ndk`.

GOOD:

```bash
rustc --version
rustup toolchain list
rustup component add rust-src --toolchain nightly
cargo install wasm-bindgen-cli --version 0.2.126 --locked
xcodebuild -version
xcodegen --version || brew install xcodegen
cargo install cargo-ndk
cargo ndk --version
```

BAD:

```text
Treat a mobile build failure as a gpux renderer regression before confirming Rust, Xcode/XcodeGen, Android SDK/NDK, and cargo-ndk.
```

Check:
For gpux-only native desktop work, do not require browser or mobile tools. For RayX browser validation that consumes gpux, verify nightly, `rust-src`, `wasm-bindgen 0.2.126`, and browser WebGPU/cross-origin isolation setup first, then run the target Cargo command here or the RayX validation command from `../RayX`.

## GPUI Concepts

- `Application` and `App` own startup, platform services, windows, menus, async execution, and global state.
- `Entity<T>` stores application state managed by GPUI. Views are entities that implement `Render`.
- `Render` builds a tree of elements each frame. `div()` is the general-purpose layout and styling element.
- Elements provide the lower-level imperative layer for custom layout, rendering, hit testing, and input.
- Actions convert keystrokes and commands into typed operations.
- `#[gpux::test]` plus the `test-support` feature provide headless test contexts and simulated platform input.

## Troubleshooting Routing

- `fxc.exe` missing, HLSL shader compilation failure, Visual Studio Build Tools, DirectX Shader Compiler, VC++ runtime DLLs, or Windows ARM64 Clang issues: read `windows-prerequisites.md`.
- .NET Android or mobile validation failures such as `XA5300`, missing Java SDK/JDK, missing `javac`, or empty `JAVA_HOME`: read the Microsoft OpenJDK 21 section in `windows-prerequisites.md`.
- Missing threaded WASM output, `SharedArrayBuffer`, `Atomics.waitAsync`, COOP/COEP headers, or `wasm_thread_entry_point`: read `multithreaded-wasm-apps.md`.
- Outdated Rust, missing XcodeGen, missing Android SDK/NDK, or missing `cargo-ndk`: use the baseline prerequisites above, then read the matching platform prerequisite reference.
- `Failed to request GPU adapter`, `No available adapters`, `wasm-bindgen` missing, or browser/WebGPU problems: read the WASM/browser sections in the platform references.
- Linux X11, Wayland, font, ALSA, Vulkan, OpenSSL, zstd, sqlite, or glib missing packages: read `macos-linux-wasm-prerequisites.md`.
