# Fast-mode timings on a wgpu desktop

PERF-4 asks for lane G on a wgpu desktop with a GPU. This records what ran on a Windows DirectX 12
machine and what ran under WSL (Vulkan through `dzn`) on the same one. The macOS tables
it is compared with are in [fast-mode.md](../fast-mode.md#measurements). Every number below comes from a
log saved in `artifacts-temp/windows-lane-g/` or `artifacts-temp/wsl-lane-g/` (git-ignored; the log's name
follows each figure).

## Machines

| | Windows (native, DX12) | WSL 2 (Vulkan) |
|---|---|---|
| GPU | NVIDIA GeForce RTX 4080 SUPER | the same GPU, as `Microsoft Direct3D12 (NVIDIA GeForce RTX 4080 SUPER)`, a discrete Vulkan 1.2.318 device from Mesa's `dzn` (Vulkan on D3D12; `vulkan-enum-dzn.log`) |
| Driver | 32.0.16.1088 (NVIDIA 610.88; `Get-CimInstance Win32_VideoController`) | 610.88 (`nvidia-smi` in WSL), Mesa dzn 25.2.8 |
| OS | Windows 11 Pro 10.0.26300.9457 | Ubuntu 24.04.4 LTS, kernel 6.6.87.2-microsoft-standard-WSL2 |
| WSL | | WSL 2.6.3.0, WSLg 1.0.71, Direct3D 1.611.1, DXCore 10.0.26100.1 |
| Commit | `f8add89`, clean tree (`perf-final-windows/run.txt`) | `862a1c3` (`f8add89` plus the one change below), clean tree |
| Lane G | ran (below) | ran, lanes C and D only (below) |

Both machines are one computer. Lane G ran with `GPUX_WGPU_ADAPTER=hardware` and
`GPUX_EXPECT_BACKEND=hardware`, `CI` unset, nothing else running. The Windows power plan (display off
after 300 s on AC) was left as it was and a small process held the display and system awake instead
(its first version had a bug and did nothing for the first minutes; lane D ran well after it was
fixed and every desktop window reported a frame).

## Windows

### Lane G

`artifacts-temp/windows-lane-g/timing.md` (copied from `artifacts-temp/testkit/g/`) and
`lane-g-stdout.log`: **418 passed, 1 failed, 10 skipped.**

| Part | Passed | Failed | Skipped | Duration |
|---|---|---|---|---|
| lane B in `local-gpu` (every spec, `@timing` included) | 184 | 0 | 0 | 301.1 s |
| lane C on the GPU, probes only | 188 | 0 | 0 | 1863.8 s |
| lane D on the GPU | 46 | 0 | 10 | 53.9 s |
| `gpux-platform` benchmarks | 0 | 1 | 0 | 283.6 s |

- Lane D's `desktop-backend` result: `windows render on hardware (NVIDIA GeForce RTX 4080 SUPER)`
  (`hardware-lane-d.log`). Its 10 skips are the macOS and Linux composition checks
  (`composition-macos-*`, `composition-wayland-*`, `composition-x11-*`), which run on their own platforms.
- Lane B's Chromium used the GPU, not WARP: its WebGPU adapter is `nvidia`/`lovelace` with
  `isFallbackAdapter` false, and WebGL2 renders on `ANGLE (NVIDIA, NVIDIA GeForce RTX 4080 SUPER ...
  Direct3D11)` (`browser-adapter-check.log`, the `local-gpu` launch flags).
- The benchmarks part failed in the `fast_mode` bench (`benches.log`, `fast_mode.log`):

  ```
  thread 'gpux-bench' panicked at crates\gpui_platform\benches\fast_mode.rs:54:9:
  fast/still-frame/off: 60 primitives over 1 frames, below 3900 a frame: the text drew no glyphs?
  ```

  Its stats show `lines_shaped: 0` with `leaves_measured: 8160`. Criterion's hardware-counter, context-switch
  and page-fault metrics are also reported unavailable on Windows. `cargo bench` stopped at the first failing
  bench, so `randomized_element_tree` and `inspector_render` never ran: **no benchmark estimate exists for
  this machine**, and lane G's own `fast-mode.md` says only `The run reported no fast/<scenario>/<mode> id.`

### Browser timing

Lane G's `@timing` specs of `scroll-layers.spec.ts`, the same page as "In the browser" of
[fast-mode.md](../fast-mode.md#in-the-browser) (60 wheel steps on a 440 by 340 canvas), on Chromium on the RTX 4080
SUPER. Three separate runs of `lane b --grep "@timing scrolling"` (`browser-timing-run1.log` to `run3.log`,
each 5 passed, 0 failed), the p95 of gpux's frame work in milliseconds, layers on / layers off, each run's
change in brackets:

| Scene | WebGPU | WebGL2 |
|---|---|---|
| cards in a scrolling `div` | 7.38 / 11.25 · 6.94 / 11.33 · 7.72 / 10.52 (-34%, -39%, -27%) | 7.36 / 10.08 · 7.25 / 9.70 · 6.97 / 11.15 (-27%, -25%, -37%) |
| rows of a `list` | 11.45 / 11.43 · 11.04 / 11.58 · 11.37 / 11.02 (+0%, -5%, +3%) | 11.85 / 10.97 · 11.19 / 10.38 · 11.59 / 11.12 (+8%, +8%, +4%) |

The two runs inside lane G (`timing.md`, other lane parts around them) agree in direction:

| Scene | WebGPU | WebGL2 |
|---|---|---|
| cards in a scrolling `div` | 9.88 / 15.03 (-34%) | 9.87 / 15.26 (-35%) |
| rows of a `list` | 14.99 / 15.97 (-6%) | 16.39 / 15.47 (+6%) |

- Frames from tiles: 56 (`div`) and 58 (list) of 276 to 322 frames in the three runs, none with layers
  off; the same counts as on the Mac.
- The slowest frame is longer with layers on in every one of the 12 on/off pairs of the three runs
  (15.47 to 21.21 ms on, 12.73 to 17.68 ms off), as on the Mac: it is the frame that rasterizes tiles. The
  one exception is lane G's own run of the `div` on WebGPU, whose layers-off maximum was 84.09 ms
  (`timing.md`), one stall.
- The absolute p95 is 2 to 3 times the Mac's (7 to 12 ms against 2.75 to 4.3 ms) with layers on and off
  alike. It was not investigated.
- The uniform list's timing check (held under the 60 Hz budget in lane G) read p95 2.74 ms over 376 frames
  (`timing.md`).

### The `fast_mode` suite

`cargo run -p gpux-testkit -- perf fast-mode --name final-windows` (`perf-fast-mode-final-windows.log`,
`perf-final-windows/`): **failed in the same place as lane G's benchmarks run**, on its first scenario, so
there is no table to compare the modes with:

```
fast/still-frame/off: 60 primitives over 1 frames, below 3900 a frame: the text drew no glyphs?
perf fast-mode: the bench reported no id; see ...\perf\final-windows\fast_mode.log
```

The check was not weakened. It is the failure `fast-mode.md` records for the Linux VM; the suite
is macOS-only until its harness draws text elsewhere, and it does not here either.

## WSL (Linux, Vulkan through `dzn`)

### Getting a Vulkan GPU

WSL has no Vulkan driver for the GPU as installed. NVIDIA's WSL stack is D3D12 and CUDA only
(`/usr/lib/wsl/lib`: `libd3d12.so`, `libdxcore.so`, `libnvwgf2umx.so`), and Ubuntu 24.04's
`mesa-vulkan-drivers` 25.2.8 ships no `dzn` (`asahi`, `gfxstream`, `intel_hasvk`, `intel`, `lvp`,
`nouveau`, `radeon`, `virtio` only), so Vulkan listed one device, `llvmpipe` (`vulkan-enum.log`,
`wsl-version.log`). A GPU needed:

1. Mesa 25.2.8 built from source with only `-Dvulkan-drivers=microsoft-experimental` into `~/mesa-dzn`
   (Meson 1.5.2 from a clone, as Ubuntu's is 1.3.2; the apt build dependencies were the only `sudo`;
   `mesa-build.log`), selected with `VK_DRIVER_FILES=~/mesa-dzn/share/vulkan/icd.d/dzn_icd.x86_64.json`.
   `vulkaninfo` then reports `Microsoft Direct3D12 (NVIDIA GeForce RTX 4080 SUPER)`, discrete, Vulkan 1.2.318,
   driver `Dozen`; `dzn` prints that it is not a conformant implementation.
2. A change to gpux, `862a1c3` on `fix/wgpu-env-noncompliant-adapter`. wgpu hides non-conformant
   adapters (`Adapter is not Vulkan compliant, hiding adapter: Microsoft Direct3D12 (NVIDIA GeForce RTX 4080
   SUPER)`, `wgpu-probe-noncompliant-hidden.log`) unless `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER` is
   set, and gpux built its instances with `InstanceFlags::default()`, which never reads it. Without the
   change gpux saw only `llvmpipe (Gl, Cpu)` and the lane C and bench processes died with SIGSEGV (observed
   in the first, unfixed run; its logs were not kept). With it and the variable set to `1`, none did.

What ran is therefore **Vulkan translated to D3D12 on a non-conformant driver**, on the GPU the Windows run
used through DX12 directly. It is not a native Linux Vulkan driver.

### Lane G

`lane-g-stdout.log`, `timing.md`, run under `xvfb-run` (X11) with `GPUX_WGPU_ADAPTER=hardware`,
`GPUX_EXPECT_BACKEND=hardware`, `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER=1`:
**231 passed, 185 failed, 13 skipped.**

| Part | Passed | Failed | Skipped | Duration |
|---|---|---|---|---|
| lane B in `local-gpu` | 0 | 184 | 0 | 304.1 s |
| lane C on the GPU, probes only | 188 | 0 | 0 | 1884.8 s |
| lane D on the GPU | 43 | 0 | 13 | 47.9 s |
| `gpux-platform` benchmarks | 0 | 1 | 0 | 68.0 s |

- **Lane C and lane D pass on the GPU.** Lane D's `desktop-backend` result: `windows render on hardware
  (Microsoft Direct3D12 (NVIDIA GeForce RTX 4080 SUPER))` (`hardware-lane-d.log`). Its skips are 3 macOS
  and 5 Windows composition checks, 3 Wayland ones (not a Wayland session), `composition-x11-overlay` (no
  compositing manager under Xvfb) and `platform-scale-change` (Linux windows take their scale from the
  display server).
- **Lane B fails in every spec**, the first one with `could not open web-svg-asset: WebGPU context not
  initialized` (`lane-b-logs/web-e2e.log`). Chromium on WSL gets no GPU adapter: with the `local-gpu` flags
  (and Playwright's headless Chromium 1217) neither WebGPU nor WebGL2 exists; with `--use-angle=gl-egl` WebGPU
  is SwiftShader and WebGL2 is `ANGLE (Mesa, llvmpipe ...)`, both software; with the Vulkan flags
  (`--use-angle=vulkan --use-vulkan=native`) again neither. The three timing runs
  (`browser-timing-run1.log` to `run3.log`) fail the same way, 0 passed, 5 failed each: **there is no
  browser p95 for WSL.**
- The benchmarks fail in the same `fast_mode` check as on Windows (`benches.log`:
  `fast/still-frame/off: 60 primitives over 1 frames, below 3900 a frame: the text drew no glyphs?`), and
  `perf fast-mode --name final-wsl` stops on it too (`perf-fast-mode-final-wsl.log`): **no `fast_mode`
  table and no benchmark estimate for WSL.**

## Findings

1. **Scroll layers still wins for the scrolling `div`, by more than on the Mac.** All six cells and both
   in-lane runs improve, by 25% to 39% (Mac: 7% to 18%), on WebGPU and WebGL2.
2. **For the list it is not a clear loss on WebGPU and still a loss on WebGL2.** WebGPU: +0%, -5%, +3% in
   three runs and -6% in lane G, within the spread. WebGL2: +8%, +8%, +4% and +6% in lane G, in all
   four runs (Mac: +19% to +28% on WebGL2, up to +32% in lane G, and +12% to +15% on WebGPU).
3. **Nothing regressed that the Mac did not show, but PERF-4 is only partly measured.** On Windows lanes B, C
   and D pass in full on the GPU; on WSL (Vulkan through `dzn`) lanes C and D pass and lane B cannot reach
   a GPU. The `fast_mode` suite fails on both platforms at its own check (the same one as on lavapipe), so
   the per-mode comparison is absent and `randomized_element_tree` and `inspector_render` have no numbers
   anywhere. What stays open: the browser timings on a Linux GPU, and the per-mode comparison off the Mac.
4. **gpux needed one change to see a Vulkan GPU under WSL** (`862a1c3`, merged into this branch with this document): honouring
   `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER`. Lane C's and lane D's result there is for `dzn`, Vulkan over
   D3D12, not for a native Linux Vulkan driver.
