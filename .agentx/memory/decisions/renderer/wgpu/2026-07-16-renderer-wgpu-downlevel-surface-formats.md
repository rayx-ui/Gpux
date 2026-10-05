---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: superseded
  created: 2026-07-16T00:55:00+02:00
  updated: 2026-07-16T00:55:00+02:00
  topics:
    - gpux
    - wgpu
    - android
    - downlevel
    - surface-formats
  supersedes: []
  superseded_by:
    - 2026-09-30-renderer-wgpu-srgb-compositing.md
---

## Decision: Configure alternate WGPU surface views only when the adapter supports them

### Context: A GLES-only Android emulator selected a valid GL adapter but panicked during `Surface::configure` because GPUI always supplied an sRGB alternate view while the adapter lacked `DownlevelFlags::SURFACE_VIEW_FORMATS`.

### Alternatives considered: Require Vulkan-capable Android devices; force GL without changing surface configuration; suppress the validation error; remove sRGB surface views on every platform; or branch surface/render-target selection on the adapter's reported downlevel capability.

### Reasoning: The adapter capability is the authoritative boundary. Full adapters retain the established linear surface plus sRGB render view, while downlevel adapters configure no alternate views and render directly to a format advertised by the surface, preferring sRGB when available. This preserves full-path color behavior and makes GLES fallback valid without an app-specific workaround.

### Trade-offs accepted: A downlevel surface that advertises no sRGB format must render directly to its available non-sRGB format, so display encoding can be less accurate than on a full adapter. Android runtime validation must keep checking logcat and DevKit because compile-only checks cannot exercise surface configuration.

### Supersedes: None.
