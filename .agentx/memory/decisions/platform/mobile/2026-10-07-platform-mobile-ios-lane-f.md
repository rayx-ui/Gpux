---
agentx:
  kind: decision
  scope: decisions/platform/mobile
  status: current
  created: 2026-10-09T17:00:00+02:00
  updated: 2026-10-09T17:00:00+02:00
  topics:
    - gpux
    - ios
    - lane-f
    - simulator
    - xcodegen
  supersedes: []
  superseded_by: []
---

## Decision: Lane F runs a static-library conformance host behind a minimal XcodeGen UIKit shell on the Simulator

### Context: iOS proof needs the scene catalog in a real UIKit app, built and run from the command line without a committed Xcode project or signing.

### Alternatives considered: A committed Xcode project; third-party cargo runners for iOS; device-only runs.

### Reasoning: `ios_host` is a staticlib that runs the catalog through `AppHost`. XcodeGen generates the shell from `tools/ios/project.yml` (the generated project is git-ignored); `gpux-testkit ios build --simulator` builds it with `xcodebuild` and no signing, and `ios run --simulator` boots a pinned Simulator, installs and launches the app with `simctl launch --console-pty`, parses the result lines, and copies failing frames from the app container. The iOS window renders to images through the shared wgpu renderer; where a Simulator has no Metal device, pixel steps report a skip with the reason. Devices run by hand with the developer's signing team and `xcrun devicectl`. The owner accepted a local lane F run as evidence while hosted CI is unavailable (2026-10-09).

### Trade-offs accepted: Lane F needs macOS with Xcode and XcodeGen. iOS stays on the Metal-specific wgpu path and CoreText (`IosTextSystem`), and the `Application` is retained after `run()` returns.

### Supersedes: None.
