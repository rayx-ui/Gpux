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
- Metal layers hold two drawables (`maximumDrawableCount = 2`), which saves a full-window IOSurface per window (12-22 MB, about 10% of the footprint). A comparison made with idle CPU clocks once read two as 10-50% more CPU on `effects-cards`; with the clocks held (`run.py --hold-clocks 3`, see the repository rules) it costs no CPU and no missed frames beyond noise (2 of 24 runs had one or two, against 1 of 24 with three), but the frame interval max rises from about 7 to 9 ms. Re-measure with clocks held before changing it; three is the one-line fallback for a GPU-bound app that drops frames.
- macOS text shaping keeps the sized Core Text font per (font, size) in `MacTextSystem`; do not clone a font per line (Core Text rebuilds its ASCII glyph cache for each copy).
