# Windows Prerequisites

Use this reference for Windows x64 and Windows ARM64 setup, including automatic installation. Lanes A, C, and D need nothing beyond a normal gpux build on Windows (WARP, the DX12 software adapter, ships with Windows); each lane's prerequisites are in [docs/testing.md](../../../../docs/testing.md#prerequisites).

## Check First

Run checks before installing:

```powershell
$env:PROCESSOR_ARCHITECTURE
winget --version
cargo --version
rustc --version
fxc /help
clang --version
java -version
javac -version
echo $env:JAVA_HOME
wasm-bindgen --version
```

For a scripted check from the Gpux repository root, use `.claude/skills/gpux/scripts/install-windows-prereqs.ps1`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\gpux\scripts\install-windows-prereqs.ps1 -CheckOnly
```

## Automatic Setup

Use the helper script only when the user asks you to install or fix prerequisites. Run `-CheckOnly` first, then use targeted flags:

```powershell
# Install common native prerequisites and add discovered tool paths.
powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\gpux\scripts\install-windows-prereqs.ps1 -InstallBuildTools -InstallVCRedist -AddFxcToPath

# Add standalone LLVM/Clang, useful on Windows ARM64 for ring.
powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\gpux\scripts\install-windows-prereqs.ps1 -InstallLLVM

# Install WASM tooling and add Cargo's bin directory to PATH.
powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\gpux\scripts\install-windows-prereqs.ps1 -InstallWasmBindgen -AddCargoBinToPath

# Full setup when the user explicitly wants everything.
powershell -NoProfile -ExecutionPolicy Bypass -File .claude\skills\gpux\scripts\install-windows-prereqs.ps1 -All
```

Expect some installers to request elevation or require a terminal or Windows restart.

## Required Native Tools

Install Visual Studio 2022 Build Tools with the Desktop development with C++ workload:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

GPUI compiles HLSL shaders and needs `fxc.exe` from the Windows SDK. Use Developer PowerShell for VS 2022 or add a Windows SDK bin directory to PATH. Common locations:

```text
C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\arm64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\x64\fxc.exe
C:\Program Files (x86)\Windows Kits\10\bin\10.0.19041.0\arm64\fxc.exe
```

## Windows ARM64

On ARM64, the `ring` crate, pulled in through `rustls`, needs Clang for ARM64 assembly. MSVC's assembler is not enough. Install either the Visual Studio C++ Clang tools component or standalone LLVM:

```powershell
winget install LLVM.LLVM
[Environment]::SetEnvironmentVariable("PATH", "C:\Program Files\LLVM\bin;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
```

Install both ARM64 and x64 Microsoft Visual C++ redistributables on Windows ARM64:

```powershell
winget install --id Microsoft.VCRedist.2015+.arm64 --exact --accept-package-agreements --accept-source-agreements
winget install --id Microsoft.VCRedist.2015+.x64   --exact --accept-package-agreements --accept-source-agreements
```

## Microsoft OpenJDK 21 for Android and Mobile Validation

Lane E needs a JDK 21 (`java`, `keytool`), and RayX's Android builds that consume gpux need Java tooling too. Install **Microsoft Build of OpenJDK 21 LTS**. Verify Java/JDK discovery before debugging gpux renderer code.

Download page:

- [Microsoft Build of OpenJDK downloads](https://learn.microsoft.com/en-us/java/openjdk/download)

Direct Windows installers:

- Windows x64: [OpenJDK 21 Windows x64 EXE](https://aka.ms/download-jdk/microsoft-jdk-21.0.11-windows-x64.exe)
- Windows ARM64: [OpenJDK 21 Windows ARM64 EXE](https://aka.ms/download-jdk/microsoft-jdk-21.0.11-windows-aarch64.exe)

Or install with Windows Package Manager:

```powershell
winget install --id Microsoft.OpenJDK.21 -e
```

After installation, open a new terminal and verify:

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

Reference docs:

- [Install Microsoft Build of OpenJDK](https://learn.microsoft.com/en-us/java/openjdk/install)

## Android NDK Tools

When gpux setup is being prepared for Android validation (lane E, or a RayX mobile build that consumes gpux), verify Android native tooling as part of setup:

```powershell
adb version
ndk-build --version
cargo install cargo-ndk
cargo ndk --version
```

Android validation needs the Android SDK, Android NDK r25 or newer, Java/JDK discovery, and `cargo-ndk`. Use `ANDROID_HOME`, `ANDROID_SDK_ROOT`, `ANDROID_NDK_HOME`, or `ANDROID_NDK_ROOT` when auto-discovery fails. With `ANDROID_NDK_HOME` set, `cargo ndk -t arm64-v8a check -p gpux-mobile` checks the gpux Android build from the Gpux repository root. Lane E (`cargo run -p gpux-testkit -- lane e`, or `android build` and `android run`) also needs `ANDROID_HOME` with its pinned SDK packages and the Android Rust targets, and reports each missing one with the command that installs it ([docs/testing.md](../../../../docs/testing.md#lane-e-android)).

## Browser and WASM

Install `wasm-bindgen-cli` when building gpux for WebAssembly or running browser-oriented `gpui_web` examples, at the version `Cargo.lock` pins (`0.2.129`; `gpux-testkit web build` refuses any other):

```powershell
cargo install wasm-bindgen-cli --version 0.2.129 --locked
[Environment]::SetEnvironmentVariable("PATH", "$HOME\.cargo\bin;" + [Environment]::GetEnvironmentVariable("PATH", "User"), "User")
```

For multithreaded gpux/RayX browser apps, also read `multithreaded-wasm-apps.md`. They require nightly Rust, `rust-src`, atomics/shared-memory linker flags, `wasm-bindgen`, and cross-origin isolation headers. Lane B also needs Node.js 22 or later with `pnpm`, and in `tools-node`: `pnpm install` and `pnpm exec playwright install chromium`. On a Windows machine without a usable GPU, run lane B in the `windows-software` profile (`$env:GPUX_WEB_PROFILE = "windows-software"`), which renders WebGPU on WARP.

On current Windows ARM64 Qualcomm/Adreno devices, Chromium browsers can report WebGPU as hardware accelerated but return no adapters unless Unsafe WebGPU is enabled:

- Edge: `edge://flags/#enable-unsafe-webgpu`
- Chrome: `chrome://flags/#enable-unsafe-webgpu`

Set Unsafe WebGPU to Enabled, fully restart the browser, then retry.

## Validate After Setup

Restart PowerShell after PATH changes, then verify:

```powershell
fxc /help
clang --version
java -version
javac -version
echo $env:JAVA_HOME
wasm-bindgen --version
cargo check -p gpux --all-targets
# Browser/WASM setup (lane B prerequisites):
cargo run -p gpux-testkit -- web build --verify
cargo run -p gpux-testkit -- lane b --list
```
