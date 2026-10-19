# ToFix: open work from the 2026-10-06 macOS CI session

These notes were the source material of the AgentX epic `macos-only-metal-port-and-ci-repair`
(`.agentx/tasks/macos-only-metal-port-and-ci-repair/`). That epic was never started (all tasks
pending, revision 0): most of its work landed on `main` through other epics. Retire or replan it
before implementing what is left.

## State On 2026-10-19

| Note | What | State |
|---|---|---|
| [01-macos-metal-renderer.md](01-macos-metal-renderer.md) | Metal renderer on the `PaintColor` contract | Code on `main` (`5321a25`, `3ad5fca`); macOS `check`, `test`, lane A, lane C (`metal` goldens) and lane D are green in CI. Follow-ups below are open |
| 02 (removed) | `gpux-zed-util` macOS build: `adopt_raw_pid` | Fixed: `5599036` adds `darwin/process.rs`, `ec24941` its tests; the import guide records the replay rule |
| 03 (removed) | WGPU dashed borders rendered solid | Fixed: `b0c1c37` fades the whole premultiplied color; scene `paint-border-strips` probes the gaps on every pixel lane; RayX merged `test/dashed-border-gaps` (`e49d9e2e`) |
| 04 (removed) | Flaky Windows `test_drop_terminates_grandchildren` | Fixed: `dff77b4` raises the pid-file deadline to 30 s; the import guide records it |

The branches, closed pull requests and RayX-Test-Suite proof described in 01 are history: gpux now
proves itself in its own lanes (`gpux-testkit`), independent of RayX, and implementation pushes
straight to `main`.

## Still Open

1. **Memory is stale.** `knowledge/platform/desktop/state.md` still says the Metal border-coverage
   port "has not been compiled or run", next to the lane C proof; `hypotheses.md` keeps it
   unverified; the 2026-09-30 sRGB-compositing decision says the Metal port is "not yet compiled or
   proven". Missing: a knowledge item that Metal-exported enums must be `#[repr(C)]` (cbindgen's
   sized-enum typedef is a C++ redefinition) and a WGPU rule that fading or masking premultiplied
   paint scales all four channels.
2. **Import guide.** `update-from-zed-gpui-closure.md` "Native Rendering" has no Metal bullet:
   upstream `shaders.metal` still uses `Hsla`, cbindgen must parse `gpux_color`'s `paint.rs`,
   Metal-exported enums stay `repr(C)`, and upstream's WGSL dash gap fades alpha only.
3. **Pixel coverage of what the port changed.** No conformance scene covers inset shadows,
   translucent or OKLab gradient stops, `pattern_slash`/`checkerboard` backgrounds, translucent
   image or emoji edges, 1 px rounded-border beading (Metal hard-codes `BORDER_GAMMA_RATIOS`), or a
   transparent window. The `metal` goldens lock in what Metal renders; they do not prove it matches
   WGPU.
4. **Apple code is not linted in CI.** CI runs clippy on Windows only. On macOS,
   `cargo clippy -p gpux-zed-util --all-targets --no-deps -- -D warnings` fails in the gpux-owned
   `darwin/process.rs` test helper (`clippy::zombie_processes`, `clippy::disallowed_methods`);
   `gpux-apple` and `gpux-macos` are clean.
5. **Build variants outside CI.** `cargo check -p gpux-apple --features runtime_shaders` and
   `--target aarch64-apple-ios` pass (2026-10-19, local), but nothing keeps them passing. No
   consumer builds `gpux-apple` for iOS (`gpui_mobile` renders through wgpu).
