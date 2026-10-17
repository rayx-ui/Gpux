# Upstream: VoiceOver tables, expanded state and range widgets

The `gpux-accesskit-macos` fork in `vendor/accesskit_macos` exists because `accesskit_macos`
0.27.1 (the latest release, 2026-10-17) gives VoiceOver no way to read some of what AccessKit
trees carry. The owner chose a vendored fork over an upstream-only fix on 2026-10-17, after the
screen-reader walkthrough found these on the macOS desktop
(`docs/measurements/screen-reader-walkthrough.md`). This file tracks the upstream change that
makes the fork unnecessary.

| | |
|---|---|
| Fork | `accesskit_macos` 0.27.1 renamed `gpux-accesskit-macos` 0.27.1-gpux.1, Rust name kept |
| Changed files | `src/node.rs`, `src/event.rs` (the `gpux fork:` comments mark each change) |
| Upstream issue | Not filed yet |
| Pull request | Not opened yet |

What the fork changes, each found with VoiceOver on the conformance `accessibility` scene:

1. **Tables expose their rows and cells.** `accessibilityRows` answered only for containers
   with selectable children, so VoiceOver announced a plain `Role::Table` as "empty". A table's
   rows (its `Row` children, also inside row groups) are now its rows, a row gives
   `accessibilityIndex`, and a cell gives `accessibilityRowIndexRange` and
   `accessibilityColumnIndexRange` (from `row_index`/`column_index`, else its position).
2. **Expanded state.** `isAccessibilityExpanded` reports `Node::is_expanded`, and a change posts
   `AXExpandedChanged`; VoiceOver now says "collapsed"/"expanded".
3. **Range widgets are adjusted by increment, and read by value.** Slider, spin button, scroll
   bar, progress indicator and meter answer `accessibilityValueDescription` with their value text
   (else their number), which VoiceOver reads instead of a percentage of the range (a spin button
   at 1 of 1–9 was "0%"). A range widget that supports increment or decrement no longer allows
   `setAccessibilityValue:`: VoiceOver set the value, read it back before the app's next frame
   updated it, and kept raising its step, so one adjustment jumped to the range's end.
4. **Disabled controls are dimmed.** A disabled button, check box, radio button, switch, link,
   menu item or tab allows `accessibilityPerformPress` (the press does nothing): VoiceOver says
   "dimmed" only for a control with a press action, and "no action available" otherwise.

The conformance check `screen-reader-macos-voiceover` (lane D, macOS) proves all four through
the window's `accessibilityChildren`; against the unmodified crate it fails ("the Scores table
shows VoiceOver 0 rows, not 2").

When an `accesskit_macos` release ships these: delete `vendor/accesskit_macos`, set the workspace
dependency back to the crates.io `accesskit_macos`, and keep `screen-reader-macos-voiceover`
passing. gpux itself also sets ARIA's default orientation (horizontal) on sliders that set none
(`crates/gpui/src/elements/div.rs`, test group `a11y_aria_defaults`), which is not part of the
fork.
