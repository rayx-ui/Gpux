---
agentx:
  kind: knowledge
  scope: knowledge/platform/desktop
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-06T05:50:00+02:00
  topics:
    - windows
    - subpixel-text
    - metal
    - accesskit
  supersedes: []
  superseded_by: []
---
# Desktop Knowledge

- Snapshotting dual-source blending at window construction avoids both an unconditional subpixel claim and a paint-time `RefCell<WgpuRenderer>` borrow when GPUI asks whether LCD text is supported.
- Converting `WM_GETOBJECT` results and raising UI Automation events can re-enter the window procedure, so adapter borrows must be released first.
- Metal shader fixes must keep packed Rust buffer ABIs and convert cbindgen-exported array fields to Metal vector types in shader helpers at use sites.
- The 2026-08-07 Zed import left Wayland `client.rs`/`window.rs` at the older closure state and the popup follow-up was hand-ported onto them, so default-feature `gpux-linux` stopped compiling until the 2026-10-05 re-import.
- `gpux-platform`, like upstream `gpui_platform`, enables no Linux windowing backend by default, and the workspace declares `gpux-linux` with `default-features = false`. A consumer that omits `gpui_platform/wayland` and `gpui_platform/x11` compiles neither backend and silently runs on the headless client: no window, a no-op `on_request_frame`, and no `render_to_image`. Input and state still update, but nothing redraws, so DevKit snapshots stay at the startup frame.
