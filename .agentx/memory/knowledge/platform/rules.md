---
agentx:
  kind: rules
  scope: knowledge/platform
  status: current
  created: 2026-06-24T14:05:00+02:00
  updated: 2026-10-16T09:00:00+02:00
  topics:
    - platform
    - wasm-timing
    - popups
  supersedes: []
  superseded_by: []
---
# Platform Rules

- Browser-active Rust code must not use `std::time::SystemTime` or `std::time::Instant` on `wasm32-unknown-unknown`; use `js_sys::Date::now()` for wall-clock time and `gpux::Instant` (or `instant`/`web_time`) for elapsed time.
- Implement anchored popups only on backends with a complete native lifecycle; return `PopupNotSupportedError` elsewhere rather than silently creating a top-level window.
- Make platform text-capability answers reflect negotiated renderer features.
- Metal layers hold three drawables (`maximumDrawableCount = 3`). Two saved a full-window IOSurface (about 16 MB, 10% of the footprint) but read 10-50% more main-thread CPU and slightly more missed frames in `effects-cards` and `dashboard-one-panel`; do not lower it without re-measuring those.
- macOS text shaping keeps the sized Core Text font per (font, size) in `MacTextSystem`; do not clone a font per line (Core Text rebuilds its ASCII glyph cache for each copy).
