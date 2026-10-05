---
agentx:
  kind: index
  scope: knowledge/platform
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-05T23:00:00+02:00
  topics:
    - platform
    - gpui-platform
    - web
    - desktop
    - mobile
  supersedes: []
  superseded_by: []
---
# Platform Domain Index

Platform crates behind `gpui_platform`: browser (`gpui_web`), desktop (`gpui_windows`, `gpui_macos`, `gpui_linux`), and mobile (`gpui_mobile`).

- [state](state.md) · [knowledge](knowledge.md) · [rules](rules.md) · [hypotheses](hypotheses.md)

## Subdomains

- [web](web/INDEX.md): `gpui_web` and the WASM side of `gpui_platform`: threaded startup, browser windows, clipboard, assets, HTML islands, and HTML elements.
- [desktop](desktop/INDEX.md): Windows WGPU/DX12, macOS Metal, Linux Wayland/X11, native AccessKit, popups, desktop text systems.
- [mobile](mobile/INDEX.md): `gpui_mobile` Android and iOS.
