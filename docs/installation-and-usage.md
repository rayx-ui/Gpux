# Installation and usage

`gpux` is the RayX-maintained GPUI fork. The repository keeps Zed's GPUI closure paths (`crates/gpui`, `crates/gpui_platform`, ...); the main package and import crate are named `gpux`, and the other crates publish as `gpux-*` packages that keep their GPUI Rust names (the [README](../README.md) lists them).

## Add gpux to a Rust project

From crates.io:

```toml
[dependencies]
gpux = "0.3"
gpux-platform = "0.3"

[dev-dependencies]
gpux = { version = "0.3", features = ["test-support"] }
gpux-platform = { version = "0.3", features = ["test-support"] }
```

From a sibling checkout of this repository, for co-development:

```toml
[dependencies]
gpux = { path = "../Gpux/crates/gpui" }
gpui_platform = { package = "gpux-platform", path = "../Gpux/crates/gpui_platform" }
```

Enable platform features as needed:

- `gpui_platform/font-kit` for macOS font-kit support
- `gpui_platform/wayland` and `gpui_platform/x11` for Linux windowing backends
- `gpux/test-support` and `gpui_platform/test-support` for GPUI tests
- `gpux/inspector`, `gpux/screen-capture`, `gpux/leak-detection`, and `gpux/input-latency-histogram` for targeted diagnostics or optional capabilities

Then import from `gpux::{...}` as normal.

## Minimal native app

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

## Examples

The crate examples are useful starting points:

```bash
cargo run -p gpux --example hello_world
cargo run -p gpux --example input
cargo run -p gpux --example uniform_list
```

## Local development checks

The workspace default member path is `crates/gpui`, whose Cargo package is `gpux`, so simple Cargo commands focus on the main framework crate.

```bash
cargo metadata --no-deps
cargo fmt --check
cargo check -p gpux --all-targets
cargo test -p gpux --features test-support
```

For broader changes, especially platform, renderer, shader, or support-crate work, also check the touched packages directly:

```bash
cargo check -p gpux-platform --all-targets
cargo check -p gpux-wgpu-renderer --all-targets
cargo check -p gpux-windows --all-targets
cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features
```

Some package checks depend on the host platform and installed native SDKs. The web check disables
default features because the default multithreaded build needs nightly `-Z build-std`, which
`gpux-testkit web build` provides.

## Testing

gpux proves itself in its own test lanes, run by `gpux-testkit` from the repository root:

```bash
cargo run -p gpux-testkit -- lane <lane>... [--list] [--scene <id>] [--grep <pattern>] [--headed]
```

Lane A covers behaviour on the test platform, B a real browser (WASM on WebGPU and WebGL2), C
offscreen pixels, D real desktop windows, E Android, F iOS on the Simulator (macOS only), and G
the local GPU with timing and benchmarks. [testing.md](testing.md) describes each lane, its
prerequisites, its environment variables and its CI cadence, and
[LINUX-WSL.md](../LINUX-WSL.md) runs the Linux lanes under WSL.

## GPUI concepts

GPUI combines retained application state with immediate-style view rendering:

- `Application` and `App` own process-wide startup, platform services, windows, menus, async execution, and global state.
- `Entity<T>` stores application state managed by GPUI. Views are entities that implement `Render`.
- `Render` builds a tree of elements each frame. `div()` is the general-purpose building block for layout, styling, and composition.
- Elements provide the lower-level imperative layer for custom layout, rendering, hit testing, input, and highly specialized UI.
- Actions convert keystrokes and commands into typed operations that can be dispatched through the UI.
- `#[gpux::test]` and the `test-support` feature provide headless test contexts and simulated platform input.

## Browser / WASM Tooling

If you build gpux for WebAssembly or run browser-oriented `gpui_web` examples, install `wasm-bindgen-cli` and make sure Cargo's bin directory is on `PATH`.

The threaded browser build (`gpux-testkit web build`, lanes B and G) also needs a nightly toolchain with `rust-src` and the wasm32 target, and its Playwright specs need Node.js 22 or later with `pnpm`:

```bash
rustup toolchain install nightly-2026-06-23 --component rust-src --target wasm32-unknown-unknown
cd tools-node
pnpm install
pnpm exec playwright install chromium
```

### Windows ARM64 Chromium browsers

On current Windows ARM64 Qualcomm/Adreno devices, Chromium-based browsers can report WebGPU as hardware accelerated but still return no adapters unless the browser's Unsafe WebGPU flag is enabled.

If a gpux browser build fails with messages such as `Failed to request GPU adapter` or `No available adapters`, open one of these addresses directly in the browser address bar:

- Edge: `edge://flags/#enable-unsafe-webgpu`
- Chrome: `chrome://flags/#enable-unsafe-webgpu`

Set **Unsafe WebGPU** to **Enabled**, then fully restart the browser and try again.

### Install `wasm-bindgen-cli`

Install the version `Cargo.lock` pins; `gpux-testkit web build` refuses any other:

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

### Add Cargo's bin directory to `PATH`

Windows PowerShell:

```powershell
[Environment]::SetEnvironmentVariable("PATH", "$HOME\.cargo\bin;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
```

macOS / Linux:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Restart your terminal after updating `PATH`, then verify:

```bash
wasm-bindgen --version
```

## Windows (x64 and ARM64)

### 1. Visual Studio 2022 Build Tools

Install **Visual Studio 2022 Build Tools** with the **Desktop development with C++** workload. This provides the MSVC compiler, Windows SDK, and the DirectX Shader Compiler (`fxc.exe`) needed by GPUI.

```powershell
# Install via winget (Build Tools only, no IDE)
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### 2. Windows SDK on PATH

GPUI compiles HLSL shaders during build and needs `fxc.exe` from the Windows SDK. Plain PowerShell does not set the SDK paths. Use **Developer PowerShell for VS 2022** or add the SDK bin directory to PATH permanently.

`fxc.exe` is located under `C:\Program Files (x86)\Windows Kits\10\bin\<sdk-version>\<arch>\`. Common default paths:

```text
c:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\fxc.exe
c:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\arm64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\arm64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\fxc.exe
```

Add the directory matching your architecture and SDK version to PATH permanently:

```powershell
# ARM64:
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
# x64:
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
```

Restart PowerShell after adding to PATH.

### 3. Clang (ARM64 only)

The [`ring`](https://github.com/briansmith/ring) cryptography crate, a transitive dependency of GPUI via `rustls`, needs Clang to compile ARM64 assembly. MSVC's assembler does not support the format `ring` uses. This is **not required on x64**.

Option A: use the Clang bundled with Visual Studio.

Install the **C++ Clang tools for Windows** component in the Visual Studio Installer, then add it to your PATH:

```powershell
# For VS 2022 Build Tools (ARM64 host):
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\ARM64\bin;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")

# For VS 2022 Community (ARM64 host):
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\ARM64\bin;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
```

Option B: install LLVM standalone.

```powershell
winget install LLVM.LLVM
```

The installer does **not** add Clang to PATH automatically. Add it permanently:

```powershell
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files\LLVM\bin;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
```

Restart PowerShell and verify both tools are found:

```powershell
clang --version
fxc /help
```

### 4. Microsoft Visual C++ Redistributable

GPUI native dependencies may require VC++ runtime DLLs at load time. On Windows ARM64, install both ARM64 and x64 redistributables.

```powershell
# Install (or update) both runtimes
winget install --id Microsoft.VCRedist.2015+.arm64 --exact --accept-package-agreements --accept-source-agreements
winget install --id Microsoft.VCRedist.2015+.x64   --exact --accept-package-agreements --accept-source-agreements
```

If already installed, repair both packages:

```powershell
Invoke-WebRequest https://aka.ms/vs/17/release/vc_redist.arm64.exe -OutFile "$env:TEMP\vc_redist.arm64.exe"
Invoke-WebRequest https://aka.ms/vs/17/release/vc_redist.x64.exe   -OutFile "$env:TEMP\vc_redist.x64.exe"

Start-Process "$env:TEMP\vc_redist.arm64.exe" -ArgumentList "/repair /quiet /norestart" -Wait
Start-Process "$env:TEMP\vc_redist.x64.exe"   -ArgumentList "/repair /quiet /norestart" -Wait
```

Restart Windows after install or repair.

### 5. Microsoft OpenJDK 21 for Android

Lane E builds the Android conformance host and needs a JDK 21 (`java`, `keytool`); its other prerequisites are in [testing.md](testing.md#lane-e-android). **Microsoft Build of OpenJDK 21 LTS** works.

Download page:

- [Microsoft Build of OpenJDK downloads](https://learn.microsoft.com/en-us/java/openjdk/download)

Direct Windows installers:

- Windows x64: [OpenJDK 21 Windows x64 EXE](https://aka.ms/download-jdk/microsoft-jdk-21.0.11-windows-x64.exe)
- Windows ARM64: [OpenJDK 21 Windows ARM64 EXE](https://aka.ms/download-jdk/microsoft-jdk-21.0.11-windows-aarch64.exe)

Or install with Windows Package Manager:

```powershell
winget install --id Microsoft.OpenJDK.21 -e
```

After installation, open a new terminal and verify Java, the Java compiler, and `JAVA_HOME`:

```powershell
java -version
javac -version
echo $env:JAVA_HOME
```

If `JAVA_HOME` is still empty, set it to the installed JDK folder. Adjust the folder name if the installed patch version differs:

```powershell
[Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Microsoft\jdk-21.0.11.10-hotspot", "User")
```

Reopen the terminal after setting `JAVA_HOME`.

RayX consumers only: RayX's .NET for Android builds fail with `XA5300` when they cannot locate the Android or Java SDK. The same JDK fixes it; if .NET still cannot discover it, pass it explicitly with `dotnet build -f net10.0-android -p:JavaSdkDirectory="$env:JAVA_HOME"` (see [.NET for Android dependency installation](https://learn.microsoft.com/en-us/dotnet/android/getting-started/installation/dependencies) and [XA5300](https://learn.microsoft.com/en-us/dotnet/android/messages/xa5300)).

Reference docs:

- [Install Microsoft Build of OpenJDK](https://learn.microsoft.com/en-us/java/openjdk/install)

## macOS

Install Xcode Command Line Tools, which provide Clang and the Metal SDK:

```bash
xcode-select --install
```

GPUI uses Metal for rendering on macOS. Xcode provides the Metal compiler and frameworks automatically.

## Linux (Ubuntu / Debian)

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential gcc g++ clang cmake make \
    libxcb-dev libxkbcommon-x11-dev libwayland-dev \
    libfontconfig-dev libasound2-dev \
    libvulkan1 libssl-dev libzstd-dev \
    libsqlite3-dev libglib2.0-dev \
    curl git jq
```

## Linux (Fedora / RHEL)

```bash
sudo dnf install -y \
    gcc gcc-c++ clang cmake make \
    libxcb-devel libxkbcommon-x11-devel wayland-devel \
    fontconfig-devel alsa-lib-devel \
    vulkan-loader openssl-devel libzstd-devel \
    sqlite-devel glib2-devel \
    curl git jq perl-FindBin perl-IPC-Cmd perl-File-Compare perl-File-Copy
```

## Linux (Arch)

```bash
sudo pacman -S \
    gcc clang cmake make \
    libxcb libxkbcommon-x11 wayland \
    fontconfig alsa-lib \
    vulkan-icd-loader openssl zstd \
    sqlite glib2 \
    curl git jq
```
