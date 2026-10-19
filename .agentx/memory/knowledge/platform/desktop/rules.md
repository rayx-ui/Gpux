---
agentx:
  kind: rules
  scope: knowledge/platform/desktop
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-19T12:38:00+02:00
  topics:
    - windows
    - dx12
    - subpixel-text
    - accesskit
    - popups
    - emoji-fonts
    - metal
  supersedes: []
  superseded_by: []
---
# Desktop Rules

- Keep Windows on the WGPU/DX12 renderer path; do not reintroduce the DirectX renderer/device/text stack or a Vulkan/GL Windows backend policy as a fallback.
- Report Windows subpixel text only when `WgpuRenderer` negotiated dual-source blending, from the snapshot taken at window construction.
- Preserve native AccessKit activation/action/deactivation wiring and platform bounds/focus updates; on Windows release adapter borrows before converting `WM_GETOBJECT` results or raising UI Automation events.
- Keep Wayland popup parent/grab/configure/reposition/teardown semantics together.
- Keep normalized `Segoe UI Emoji`, `Noto Color Emoji`, and `Apple Color Emoji` recognition in Cosmic Text; do not classify `Segoe UI Symbol` as color emoji.
- Keep 1 px rounded borders even: `paint-contract-border-beading` stays within RayX's beading limits (`Step::EvenRing`) on every pixel lane, Metal and WGPU alike. A change to either renderer's border edge coverage reruns it on lanes B, C and D.
- Before blaming code for lane D "no frame within 10s" on macOS, check the session: a locked screen (`CGSSessionScreenIsLocked`) or a sleeping display presents no frames, and the baseline `platform-open-present` fails the same way.
- On a Mac, run the Linux lane D checks in the Lima VM `gpux` (Ubuntu 24.04): Wayland under headless sway (`WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1`), X11 under `xvfb-run` (with `xcompmgr` for the composition overlay). Headless Weston advertises no `wl_seat`, which gpux requires.
- Never give a Wayland surface handed to a native producer a `wp_viewport` or buffer scale of GPUI's: the producer may create its own, and a second viewport is a protocol error.
- Prove desktop window behavior in lane D (`desktop_host`): presenting, resizing, scale changes, clipboard, OS input, and close, in real windows on each OS.
- On Windows, never clear a native surface visual's children: a producer (WebView2) adds its own; remove only what gpux nested. Lane D content set with `SetContent` cannot catch this, so run the WebView2 example after DirectComposition changes.
