# macOS, Linux, and WASM Prerequisites

Use this reference for non-Windows setup and browser/WASM tooling.

Run gpux validation commands from the Gpux repository root and RayX Test Suite commands from `../RayX`.

## macOS

Install Xcode 15 or newer, Xcode Command Line Tools, and XcodeGen:

```bash
xcode-select --install
brew install xcodegen
```

GPUI uses Metal on macOS. Xcode provides the Metal compiler, frameworks, and Clang. After installation, validate:

```bash
xcodebuild -version
xcodegen --version
clang --version
cargo check -p gpux --all-targets
```

iOS validation requires macOS plus Apple's `xcrun`/Xcode toolchain. Do not treat iOS target failures from Windows or Linux hosts as gpux code failures.

## Rust

Use Rust 1.95 or newer (the gpux workspace `rust-version`; `rust-toolchain.toml` pins 1.95.0):

```bash
rustc --version
cargo --version
```

If Rust is older than 1.95, update with `rustup update` before running gpux or RayX validation.

## Linux: Ubuntu and Debian

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential gcc g++ clang cmake make \
    libxcb1-dev libxkbcommon-x11-dev libwayland-dev \
    libfontconfig-dev libasound2-dev \
    libvulkan1 libssl-dev libzstd-dev \
    libsqlite3-dev libglib2.0-dev \
    curl git jq
```

## Linux: Fedora and RHEL

```bash
sudo dnf install -y \
    gcc gcc-c++ clang cmake make \
    libxcb-devel libxkbcommon-x11-devel wayland-devel \
    fontconfig-devel alsa-lib-devel \
    vulkan-loader openssl-devel libzstd-devel \
    sqlite-devel glib2-devel \
    curl git jq perl-FindBin perl-IPC-Cmd perl-File-Compare perl-File-Copy
```

## Linux: Arch

```bash
sudo pacman -S \
    gcc clang cmake make \
    libxcb libxkbcommon-x11 wayland \
    fontconfig alsa-lib \
    vulkan-icd-loader openssl zstd \
    sqlite glib2 \
    curl git jq
```

## WASM Tooling

Install `wasm-bindgen-cli` for gpux WebAssembly builds or `gpui_web` examples. gpux and RayX currently use `0.2.126`:

```bash
cargo install wasm-bindgen-cli --version 0.2.126 --locked
```

Make sure Cargo's bin directory is on PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Persist that line in the user's shell profile when they want a permanent setup. Restart the terminal, then verify:

```bash
wasm-bindgen --version
```

For multithreaded gpux/RayX browser apps, also read `multithreaded-wasm-apps.md`. They require nightly Rust, `rust-src`, atomics/shared-memory linker flags, `wasm-bindgen`, and cross-origin isolation headers.

## Android Tooling

For Android or RayX mobile validation that consumes gpux, install the Android SDK, Android NDK r25 or newer, and `cargo-ndk`:

```bash
cargo install cargo-ndk
cargo ndk --version
```

Verify the SDK/NDK with the tools available on the host, such as:

```bash
adb version
ndk-build --version
```

Use `ANDROID_HOME`, `ANDROID_SDK_ROOT`, `ANDROID_NDK_HOME`, or `ANDROID_NDK_ROOT` when auto-discovery fails. With `ANDROID_NDK_HOME` set, `cargo ndk -t arm64-v8a check -p gpux-mobile` checks the gpux Android build from the Gpux repository root.

## Browser WebGPU

If a browser build fails with `Failed to request GPU adapter` or `No available adapters`, check browser WebGPU support and flags. On Windows ARM64 Chromium, Unsafe WebGPU may need to be enabled:

- Edge: `edge://flags/#enable-unsafe-webgpu`
- Chrome: `chrome://flags/#enable-unsafe-webgpu`

## Validate After Setup

```bash
cargo metadata --no-deps
cargo fmt --check
cargo check -p gpux --all-targets
# From ../RayX:
cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu
```
