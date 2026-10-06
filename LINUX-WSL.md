# Linux Validation on WSL

This guide sets up Ubuntu 24.04 under WSL 2 on a Windows machine so gpux and RayX can be validated
on Linux. It covers four checks:

- gpux's CI checks: format, check, web check, Clippy, tests, and packaging.
- RayX's `rust-validation` CI job: format, Clippy, workspace tests, and the Lab and Test Suite WASM
  builds.
- RayX's `wasm-playwright` CI job: the headed Test Suite on WebGPU.
- The native Linux Test Suite in a WSLg window.

Verified on 2026-10-06 with Ubuntu 24.04.4, WSL 2.6.3, Gpux `main` at `7300789`, and RayX
`master` at `9f4d2a1d`. From cold caches, every command in sections 5 and 6 passed: 1,144 gpux
tests, 2,478 RayX tests, a headed WebGPU spec at both DPRs, and a DevKit drag in the native
Test Suite.

Steps marked **(sudo)** need the machine owner's password. An agent should stop at those steps and
ask the owner to run the command; it must not enter the password itself.

## 1. WSL distribution

From Windows PowerShell:

```powershell
wsl --install -d Ubuntu-24.04
```

WSLg is part of current WSL, so Linux windows open on the Windows desktop. Check with
`wsl --version`; WSL 2.0 or later is required.

Build output grows past 100 GB, and the WSL virtual disk (`ext4.vhdx`) grows with it but never
shrinks on its own. Section 7 compacts it after cleanup. Do not enable sparse disks with
`wsl --manage <distribution> --set-sparse true --allow-unsafe`: WSL 2.6 disabled sparse disks
because they can corrupt the distribution's disk.

Keep the repositories and build output on the Linux file system (`~`), not under `/mnt/c` or
`/mnt/d`. Windows drives are much slower from Linux, and Windows checkouts carry CRLF line endings.

## 2. System packages (sudo)

Run these in the Ubuntu shell. The first command installs the compilers and libraries that gpux and
RayX build against. The second installs the shared libraries and fonts that Playwright's Chromium
needs; it is the list printed by `pnpm exec playwright install-deps --dry-run chromium` for Ubuntu
24.04.

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential pkg-config git curl jq cmake \
    clang llvm lld libclang-dev \
    libssl-dev libzstd-dev libsqlite3-dev libglib2.0-dev \
    libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libfontconfig1-dev libfreetype-dev libasound2-dev \
    libvulkan1 mesa-vulkan-drivers xdg-utils
sudo apt-get install -y --no-install-recommends \
    libasound2t64 libatk-bridge2.0-0t64 libatk1.0-0t64 libatspi2.0-0t64 libcairo2 libcups2t64 \
    libdbus-1-3 libdrm2 libgbm1 libglib2.0-0t64 libnspr4 libnss3 libpango-1.0-0 libx11-6 libxcb1 \
    libxcomposite1 libxdamage1 libxext6 libxfixes3 libxkbcommon0 libxrandr2 xvfb \
    fonts-noto-color-emoji fonts-unifont libfontconfig1 libfreetype6 xfonts-cyrillic \
    xfonts-scalable fonts-liberation fonts-ipafont-gothic fonts-wqy-zenhei fonts-tlwg-loma-otf \
    fonts-freefont-ttf
```

What depends on them:

- `clang` and `llvm` (`llvm-ar`): C dependencies of the RayX WASM builds, including Lab.
- `libclang-dev`: RocksDB bindings, which `cargo test --workspace --all-features` builds.
- `mesa-vulkan-drivers`: lavapipe, the Vulkan driver the native Linux build renders through. WSL
  has no hardware Vulkan driver.

Node.js 22 or later is also required. Ubuntu 24.04's own `nodejs` package is version 18, so install
Node 22 from NodeSource if `node --version` reports less than 22 **(sudo)**:

```bash
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt-get install -y nodejs
```

## 3. Rust and pnpm

These need no sudo:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
mkdir -p ~/.local/bin
corepack enable --install-directory ~/.local/bin pnpm
```

Notes:

- The pinned 1.95.0 toolchain installs itself from each repository's `rust-toolchain.toml` on the
  first `cargo` command.
- RayX's WASM builds use `cargo +nightly` with `-Z build-std`, hence the nightly toolchain with
  `rust-src`.
- `wasm-bindgen-cli` must match the generated manifests exactly: 0.2.129.

Make sure `~/.local/bin` comes before `/mnt/c/...` in `PATH`. WSL appends Windows `PATH`
entries, and a Windows `pnpm` would otherwise run instead. Ubuntu's `~/.profile` prepends
`~/.local/bin` in new login shells once the folder exists. Check with `which pnpm`, which must print
`~/.local/bin/pnpm`.

## 4. Repositories

RayX resolves its siblings by relative path (`../Gpux`, `../RayX.Dtcg`, `../RayX.Reactive`,
`../RayX.SignalR`), so all five repositories go side by side in one folder.

To validate local, unpushed work, clone the Windows checkouts. `safe.directory` is needed because
Windows owns those folders:

```bash
mkdir -p ~/rayx && cd ~/rayx
for repo in Gpux RayX RayX.Dtcg RayX.Reactive RayX.SignalR; do
    git -c safe.directory='*' clone /mnt/d/Work/projects/RayX/$repo $repo
done
```

Each clone checks out the branch that is current in its Windows checkout. To pick up later commits,
fetch and reset to the branch under test:

```bash
for repo in Gpux RayX RayX.Dtcg RayX.Reactive RayX.SignalR; do
    git -C ~/rayx/$repo -c safe.directory='*' fetch origin
done
git -C ~/rayx/RayX reset --hard origin/master
```

To validate pushed work instead, clone `https://github.com/rayx-ui/<repository>.git`. The
repositories are private, so authenticate first, for example with `gh auth login`.

Install RayX's Playwright packages and Chromium:

```bash
cd ~/rayx/RayX/tools-node
pnpm install --frozen-lockfile
pnpm exec playwright install chromium
```

## 5. gpux validation

From `~/rayx/Gpux`, mirroring `.github/workflows/ci.yml`. The excluded crates are the vendored wgpu
fork, which gpux does not lint or test.

```bash
EXCLUDES="--exclude gpux-wgpu --exclude gpux-wgpu-core --exclude gpux-wgpu-hal \
  --exclude gpux-wgpu-core-deps-apple --exclude gpux-wgpu-core-deps-emscripten \
  --exclude gpux-wgpu-core-deps-wasm --exclude gpux-wgpu-core-deps-windows-linux-android"
cargo fmt --check
cargo check --workspace --all-targets
cargo check -p gpux-web --target wasm32-unknown-unknown --no-default-features
cargo clippy --workspace --all-targets --no-deps $EXCLUDES -- -D warnings
cargo test --workspace --no-fail-fast $EXCLUDES -- --test-threads=1
cargo package --workspace
```

## 6. RayX validation

From `~/rayx/RayX`, mirroring `.github/workflows/pr-test.yml`.

The `rust-validation` job:

```bash
cargo fmt --check
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
cargo run -p xtask -- app apps/lab build wasm
cargo run -p xtask -- app apps/test_suite build wasm --no-default-features --features rayx_diagnostics
```

The `wasm-playwright` job runs the headed Test Suite on WebGPU in a WSLg Chromium window:

```bash
cargo run -p xtask -- app apps/test_suite test wasm --headed --webgpu \
    --no-default-features --features rayx_diagnostics
```

Add `--playwright-test test_suite/<spec>.spec.ts` (repeatable) to run selected specs. For two
specs that need opt-in fixtures, add their features to `--features`:

- `rayx-indexeddb-startup` needs `indexeddb-storage-fixture`.
- `rayx-property-grid` needs `navigation-transition-fixture`.

Things to expect on WSL:

- WebGPU runs on SwiftShader, Chromium's software renderer. WSLg offers hardware OpenGL but no
  hardware Vulkan. `tools-node/playwright.config.ts` detects WSL and passes
  `--use-angle=swiftshader --use-vulkan=swiftshader`. Without them, WebGPU canvases never reach
  the page or its screenshots.
- Software rendering is slow. The full suite (two DPR projects, two workers) takes several
  hours.
- Specs that assert latency or frame-timing limits fail on WSL, typically "under 500 ms" or "p95
  under 1 s". So do specs that sample state a fixed delay after input. They pass on a GPU.
- To separate CPU contention from real failures, rerun the failures with
  `RAYX_PLAYWRIGHT_WORKERS=1` in the environment. Then confirm any that still fail on Windows
  with a real GPU before treating them as defects.

The native Test Suite opens a WSLg window. It uses Wayland by default; start it with
`env -u WAYLAND_DISPLAY` to use X11 instead. `--features devkit` starts the RayX DevKit, so an
agent can drive the app over JSON-RPC on `127.0.0.1` (port `RAYX_DEVKIT_PORT`).

```bash
cargo run -p xtask -- app apps/test_suite run linux --features devkit
```

Startup prints harmless libEGL and Mesa warnings, from EGL failing to probe a GPU under WSL;
rendering uses Vulkan through lavapipe.

The `rayx` crate enables gpux-platform's `wayland` and `x11` features. If a native app on Linux
renders once and then never redraws, or DevKit reports `render_to_image not implemented`, check
those features first: without them gpux runs on its headless platform.

## 7. Cleanup

Build output is the bulk of the disk use: about 105 GB after sections 5 and 6 (29 GB in
`Gpux/target`, 76 GB in `RayX/target`). Delete it when you are done:

```bash
rm -rf ~/rayx/*/target ~/rayx/RayX/artifacts
```

Then give the space back to Windows. First trim the file system in Ubuntu **(sudo)**:

```bash
sudo fstrim -av
```

Then compact the virtual disk from an administrator PowerShell **(administrator)**. `wsl --shutdown`
stops every WSL distribution, including VS Code remote sessions. The `Get-ChildItem` line finds the
distribution's `ext4.vhdx`:

```powershell
wsl --shutdown
$vhdx = (Get-ChildItem "$env:LOCALAPPDATA\Packages\CanonicalGroupLimited.Ubuntu24.04LTS_*\LocalState\ext4.vhdx").FullName
"select vdisk file=`"$vhdx`"`nattach vdisk readonly`ncompact vdisk`ndetach vdisk" | Set-Content "$env:TEMP\compact-wsl.txt"
diskpart /s "$env:TEMP\compact-wsl.txt"
```

Compaction takes a few minutes for a large disk. `Optimize-VHD -Path $vhdx -Mode Full` does the
same where the Hyper-V PowerShell module is installed.
