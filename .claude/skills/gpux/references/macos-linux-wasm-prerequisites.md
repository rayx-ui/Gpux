# macOS, Linux, and WASM Prerequisites

Use this reference for non-Windows setup and browser/WASM tooling.

Run commands from the Gpux repository root. Each test lane's prerequisites are in [docs/testing.md](../../../../docs/testing.md#prerequisites).

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

iOS validation requires macOS plus Apple's `xcrun`/Xcode toolchain. Do not treat iOS target failures from Windows or Linux hosts as gpux code failures. Lane F runs iOS on a Simulator: `gpux-testkit ios build --simulator` and `ios run --simulator` (or `lane f`) need Xcode with an iOS Simulator runtime, XcodeGen (`brew install xcodegen`), and `rustup target add aarch64-apple-ios-sim`; devices are built and run by hand with a signing team ([docs/testing.md](../../../../docs/testing.md#lane-f-ios)).

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

## Linux Test Lanes

Lanes C and D render on lavapipe (`mesa-vulkan-drivers`), selected with `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json` (`lvp_icd.x86_64.json` before Mesa 25.2). Lane D also needs a display, or `xvfb-run -a -s "-screen 0 1280x1024x24"` with `WAYLAND_DISPLAY` unset, plus `xdotool` for OS input. [LINUX-WSL.md](../../../../LINUX-WSL.md) runs the lanes under WSL.

## WASM Tooling

Install `wasm-bindgen-cli` for gpux WebAssembly builds or `gpui_web` examples, at the version `Cargo.lock` pins (`0.2.129`; `gpux-testkit web build` refuses any other):

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

Make sure Cargo's bin directory is on PATH:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Persist that line in the user's shell profile when they want a permanent setup. Restart the terminal, then verify:

```bash
wasm-bindgen --version
```

For multithreaded gpux/RayX browser apps, also read `multithreaded-wasm-apps.md`. They require nightly Rust, `rust-src`, atomics/shared-memory linker flags, `wasm-bindgen`, and cross-origin isolation headers. Lane B also needs Node.js 22 or later with `pnpm`, and in `tools/browser`: `pnpm install` and `pnpm exec playwright install chromium`.

## Android Tooling

For Android validation (lane E), or a RayX mobile build that consumes gpux, install the Android SDK, Android NDK r25 or newer, and `cargo-ndk`:

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

Lane E (`cargo run -p gpux-testkit -- lane e`, or `android build [--abi x86_64|arm64-v8a]` and `android run`) also needs `ANDROID_HOME` with its pinned SDK packages, a JDK 21, and the Android Rust targets; it reports each missing one with the command that installs it ([docs/testing.md](../../../../docs/testing.md#lane-e-android)).

## Browser WebGPU

If a browser build fails with `Failed to request GPU adapter` or `No available adapters`, check browser WebGPU support and flags. On Windows ARM64 Chromium, Unsafe WebGPU may need to be enabled:

- Edge: `edge://flags/#enable-unsafe-webgpu`
- Chrome: `chrome://flags/#enable-unsafe-webgpu`

## Validate After Setup

```bash
cargo metadata --no-deps
cargo fmt --check
cargo check -p gpux --all-targets
# Browser/WASM setup (lane B prerequisites):
cargo run -p gpux-testkit -- web build --verify
cargo run -p gpux-testkit -- lane b --list
```
