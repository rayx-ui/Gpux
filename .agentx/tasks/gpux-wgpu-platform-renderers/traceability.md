---
agentx:
  kind: knowledge
  scope: repo:tasks/gpux-wgpu-platform-renderers
  status: current
  created: 2026-07-07T23:00:00+02:00
  updated: 2026-07-08T08:16:00+02:00
  topics:
    - gpux
    - wgpu
    - traceability
  supersedes: []
  superseded_by: []
---

# Traceability: GPUI WGPU Platform Renderers

## Requirement Coverage

| Requirement | Design Coverage | Implementation Coverage | Validation Coverage |
| --- | --- | --- | --- |
| FR1 Windows uses WGPU | Target Architecture | Sprint 1 Group 2, Sprint 3 Group 3 | Windows native first-frame proof; source search for `WgpuRenderer` ownership |
| FR2 Avoid DirectX renderer dependencies | Text And Atlas Design, Target Architecture | Sprint 2 Group 1, Sprint 3 Group 3, Sprint 4 Group 2 text audit | Source search for active `DirectXDevices`/`DirectXRenderer`; Windows text proof; iOS CoreText independence source assertion |
| FR3 Clean platform API boundary | Target Architecture, Migration Strategy | Sprint 3 Group 3 | No compatibility shim/proxy renderer remains |
| FR4 Explicit backend policy | Windows Backend Policy | Sprint 1 Group 3, Sprint 5 Group 2 | Diagnostics show backend set; decision record if policy changes |
| FR5 Truthful capabilities | Capability Reporting | Sprint 2 Group 2, Sprint 4 Group 2 | DevKit/runtime diagnostics; focused capability tests/checks |
| FR6 Windows lifecycle parity | Validation Design, Migration Strategy | Sprint 3 Groups 1 and 2 | Windows resize/redraw/close/device-loss diagnostics |
| FR7 Android backend preference | Current Source Shape, Target Architecture | Sprint 4 Group 1 | Android diagnostics/source proof that preference is used or API changed |
| FR8 Android surface lifecycle | Current Source Shape | Sprint 4 Groups 1 and 3 | Android native window recreate proof or exact blocker |
| FR9 iOS WGPU capabilities | Current Source Shape, Capability Reporting | Sprint 4 Groups 2 and 3 | iOS source assertions for renderer capability delegation and Metal instance creation; `aarch64-apple-ios` cargo check with local LLVM overrides; iOS runtime proof still requires macOS/Xcode/XcodeGen |
| FR10 Android/iOS full WGPU shader support | Mobile Shader Support Contract | Sprint 4 Groups 1, 2, and 3 | Android and iOS runtime proof for every `WgpuRenderer` shader-effect capability |
| FR11 Runtime diagnostics | Diagnostics And Temporary Traces | Sprint 1 Group 3, Sprint 3 Group 2, Sprint 5 Group 2 | Diagnostics fields and trace removal checks |

## Decision Points

| Decision | Default Position | When To Record A Decision |
| --- | --- | --- |
| Windows WGPU backend set | Use native `VULKAN | GL` to avoid the checked-in DirectX renderer. | If enabling WGPU DX12 or changing backend priority. |
| Windows text system | Use WGPU-compatible text, expected `CosmicTextSystem`. | If keeping any DirectWrite path that requires nontrivial rationale. |
| Android backend preference | Make the existing preference real. | If replacing/removing the API instead of wiring it. |
| DirectX code removal | Remove/decommission active DirectX renderer path. | If any DirectX module remains for a named non-renderer responsibility. |
| iOS validation | Preserve Metal/WGPU source and validate where tooling exists. | If runtime validation is deferred due host/tooling limits. |

## Validation Evidence To Record During Implementation

- Baseline WASM headed WebGPU result before code changes.
- Baseline Windows native renderer state before migration.
- Windows WGPU first-frame proof with renderer/backend diagnostics.
- Windows text proof without active DirectX GPU devices.
- Windows resize/redraw/close proof.
- Android backend-selection proof or API replacement proof.
- Android surface recreation proof or exact `adb`/device blocker.
- iOS capability proof or exact host/tooling blocker.
- iOS CoreText independence proof that `IosPlatform` constructs `IosTextSystem`, iOS text uses CoreText/font-kit CoreText loaders, and Windows text uses the separate `CosmicTextSystem` path.
- Android full WGPU shader-effect suite runtime proof with no shader/pipeline/bind-layout/logcat renderer errors.
- iOS full WGPU shader-effect suite runtime proof with no shader/pipeline/bind-layout/Metal renderer errors.
- Final WASM headed WebGPU result.
- Final formatting, focused checks/tests, and clippy result or blocker.
- Final trace-cleanup search results.
