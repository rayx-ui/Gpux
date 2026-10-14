---
agentx:
  kind: decision
  scope: decisions/renderer/wgpu
  status: current
  created: 2026-10-14T12:00:00+02:00
  updated: 2026-10-14T12:00:00+02:00
  topics:
    - gpux
    - wgpu
    - environment
    - vulkan
    - wsl
  supersedes: []
  superseded_by: []
---

# Native renderers honour one WGPU variable: the non-compliant adapter switch

## Decision: Native renderers read `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER` into the instance flags, and no other `WGPU_*` variable

### Context: wgpu hides Vulkan adapters whose driver claims no conformance unless `InstanceFlags::ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER` is set, and gpux built its instances from `InstanceFlags::default()`, which never reads the environment. Under WSL the only Vulkan driver for the GPU is Mesa's `dzn` (Vulkan on D3D12), non-conformant, so gpux saw only `llvmpipe` and its lane C processes died.

### Alternatives considered: `InstanceFlags::from_build_config().with_env()` (reads every `WGPU_*` variable); always allowing non-conformant adapters.

### Reasoning: `native_instance_flags()` in `crates/gpui_wgpu/src/wgpu_context.rs` adds the one flag when the variable is set to anything but `0`, used by `create_headless` and `instance_with_backend_policy`. Backend and adapter choice stay with gpux's own `GPUX_WGPU_*` variables, so a stray `WGPU_BACKEND` cannot move a run; an opt-in keeps a non-conformant adapter out of every default run. Tests: `cargo test -p gpux-wgpu-renderer --lib instance_flags`.

### Trade-offs accepted: A user who sets the variable gets whatever the non-conformant driver does; the browser path (`new_web`) and the atlas test instance keep default flags; the windowed path goes through `instance_with_backend_policy` and honours the variable.

### Supersedes: None.
