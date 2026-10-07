---
agentx:
  kind: rules
  scope: knowledge/platform/desktop
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-07T16:45:00+02:00
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
- Do not call the Metal border-antialiasing port done until a macOS build passes lanes C (Metal) and D with the border scenes (`paint-borders`).
- Prove desktop window behavior in lane D (`desktop_host`): presenting, resizing, scale changes, clipboard, OS input, and close, in real windows on each OS.
