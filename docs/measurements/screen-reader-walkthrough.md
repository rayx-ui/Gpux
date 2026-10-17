# Screen-reader walkthrough

Real screen readers on the conformance `accessibility` scene, run by hand by the owner following
[accessibility.md](../accessibility.md#screen-reader-walkthrough). Automated lanes prove the tree
each adapter receives; this record is what people hear.

For every platform: move through the scene from its first element with the screen reader's own
navigation (VoiceOver: VO-Right; TalkBack: swipe right; Narrator/NVDA/Orca: Tab and the reading
keys), note what is announced for each control, then perform the four actions and note what is
announced after each. "Expected" is what the AccessKit tree holds for the control.

## Record

| Field | Value |
|---|---|
| Date | 2026-10-17, 02:00–02:20 |
| gpux commit | `71ecb2b` (the s10 checkpoint) |
| Machine and OS versions | Apple M5, macOS 26.6.2 (25G83); Safari 26.6.2; Chrome 155.0.8059.80; Android emulator API 34 (arm64-v8a, TalkBack installed) |
| Who ran it | The implementing agent, at the owner's request (2026-10-17), driving VoiceOver with its own keyboard commands (System Events key presses) and reading what it spoke from VoiceOver's caption panel in screenshots (`artifacts-temp/agentx/gpux-a11y-devtools/walkthrough/`). No sound was judged. |

## Expected tree

| Control | Role | Name | State or value |
|---|---|---|---|
| Status line | status (polite live region) | its text | `clicks 0 subscribed false volume …`, then each change announced |
| Activate | button | Activate | |
| Subscribe | check box | Subscribe | unchecked, then checked after a toggle |
| Volume | slider | Volume | its value, then the adjusted value |
| Title | text field | Title | empty, then the typed text |
| Email | text field | Email | `ada@example.com`; required; described by "We never share it" |
| Fruits | list | Fruits | its items, with their positions |
| Details | button (disclosure) | Details | collapsed, then expanded; controls the "Details panel" region |
| Copies | spin button | Copies | its value |
| Delete | button | Delete | disabled |
| Code | text field | Code | |
| Scores | table | Scores | rows and cells with their positions |
| Pin | button | Pin | adds to the "Pinned" group |

## Results

Fill one table per screen reader. In "Announced" write what was spoken (or "nothing"); mark each
row OK or describe what is missing or wrong.

### VoiceOver, macOS desktop host

| Step | Announced | OK / issue |
|---|---|---|
Run with `cargo run -p gpux-conformance --no-default-features --features app-host --example desktop_host -- --show accessibility`, VoiceOver started from `VoiceOverStarter`, VO-Home then VO-Right through the window.

| Step | Announced | OK / issue |
|---|---|---|
| Window | "desktop_host, gpux accessibility, window clicks 0 subscribed false volume 5, has keyboard focus" | OK |
| Status line | "clicks 0 subscribed false volume 5" | OK |
| Activate | "Activate, button, group" | OK |
| Subscribe | "Subscribe, unchecked, checkbox" | OK |
| Volume | "50%, Volume, circular slider" | Issue: announced as a *circular* slider (the node has no orientation); value as a percentage of 0–10 |
| Title | "Title Untitled, edit text" | OK (name and placeholder) |
| Email | "Email" (label), "ada@example.com, Email, required, edit text", "We never share it" (the description's text, as the next item) | OK; the description is not read with the field |
| Fruits | "Fruits, list" | OK; no item count |
| Details | "Details, button" | Issue: no collapsed/expanded state |
| Copies | "0%, Copies, stepper" | Issue: the value (1, range 1–9) is announced as "0%" |
| Pad, Delete | "pad"; "Delete, no action available, button" | Issue: the disabled button is not announced as dimmed/unavailable, only as having no action |
| Code | "12, Code, edit text"; "Six digits" (its error message, as the next item) | OK |
| Scores | "Scores, empty, table" | Issue: the 2×2 table is announced as empty; its rows and cells are not reached as a table |
| Pin, Alpha, Beta, Shelf | "Pin, button"; "link, Alpha"; "link, Beta"; "Shelf, list" | OK |
| Activate the button (VO-Space) | "clicks 1 subscribed false volume 5" | OK: the button ran and the status line's live region was announced |
| Toggle Subscribe (VO-Space) | "clicks 1 subscribed true volume 5" | OK: toggled, announced through the live region |
| Adjust Volume | VO-Up/Down do not adjust a slider on macOS; interacting (VO-Shift-Down) and VO-Right: "clicks 1 subscribed true volume 10" | Issue: one increment moved the volume from 5 to 10 (its maximum; the step is 1) |
| Type into Title (VO-Space, then typing "Hi") | the field showed "Hi"; no echo was captured | OK (typing reached the field) |

After the fixes (the `gpux-accesskit-macos` fork and ARIA's default slider orientation, gpux
`acd2eee` and `5fbb6a2`), the same walk announced:

| Step | Announced | OK / issue |
|---|---|---|
| Volume | "5, Volume, slider"; one increment while interacting: "volume 6", the next "volume 7" | OK |
| Details | "Details, collapsed, button"; after VO-Space and moving back: "Details, expanded, button" | OK |
| Copies | "1, Copies, stepper" | OK |
| Delete | "Delete, dimmed, button" | OK |
| Scores | "Scores, table, No selection."; inside: "column 1 of 2 Ada", "column 2 of 2 3", then the second row | OK |

### VoiceOver, Safari (web host)

`gpux-testkit web build`, `gpux-testkit web serve`, `http://localhost:8000/?scene=accessibility&inspect=0` in Safari, VoiceOver on, VO-Home inside the web area then VO-Right.

| Step | Announced | OK / issue |
|---|---|---|
| Before activation | "Enable accessibility, button", then "text entry area, blank" | The button is found first. Issue: the next item is gpux's unnamed input-method textarea, which the page focuses; VoiceOver lands on it at load and VO-Right does not move past it |
| Enable accessibility (VO-Space) | the mirror appeared | OK |
| Status line | "clicks 0 subscribed false volume 5" | OK |
| Activate | "Activate, button" | OK |
| Subscribe | "Subscribe, unchecked, checkbox" | OK |
| Volume | "5, Volume, slider" | OK (value, not a percentage) |
| Title | "Title Untitled, text entry area", then the same on every further VO-Right (20 presses) | Issue: VoiceOver cannot move past the first text field; the rest of the scene is unreachable with VO-Right |
| Tab | from Title, Tab leaves for Safari's toolbar | Safari's default Tab skips the scene's other controls |
| Actions | not reached past Title | — |

After the fixes (gpux `e521be1`: the IME textarea in the mirror right after its field, unchanged
attributes left alone, a spin button's value text), the same walk announced, in order:
"Enable accessibility, button"; after VO-Space the status line, "Activate, button",
"Subscribe, unchecked, checkbox", "5, Volume, slider", "Title Untitled, text entry area",
"Email", "ada@example.com, Email We never share it, required, edit text", "list Fruits 3
items", "Apple, 1 of 3", "Pear, 2 of 3", "Plum", "Details, collapsed, button",
"Copies, stepper" (no value spoken: the mirror gives it `aria-valuetext="1"`, which VoiceOver in Safari does not read; before it said "0%"), "pad", "Delete, dimmed, button",
"12, Code, invalid data, text", "Six digits", "Scores, table, 2 columns, 2 rows",
"Ada, column 1 of 2", "3, column 2 of 2", "row 2 of 2 Max, column 1 of 2", "5, column 2 of 2",
"end of table", "Pin, button", "link, Alpha", "link, Beta", "list Shelf 5 items",
"Item 1, group, (1 of 5)". VoiceOver still announces the focused, hidden IME textarea once when
it starts on the page before activation ("text entry area, blank, has keyboard focus").

### VoiceOver, Chrome (web host)

Not run. Chrome opened to its first-run "Choose Your Search Engine" dialog in front of the scene's
tab, and the browser choice is the owner's to make; the walkthrough stopped there without
confirming anything (VoiceOver's activation press may have moved the dialog's highlighted
option; "Set as Default" was not pressed).

### VoiceOver, iOS Simulator or device

Not run: the iOS Simulator has no VoiceOver (Accessibility Inspector reads the tree, which lane
F's `screen-reader-ios-*` checks already prove), and no device was attached.

### TalkBack, Android emulator or device

Not completed. TalkBack (installed on the API 34 emulator) was turned on with
`settings put secure enabled_accessibility_services …TalkBackService` and the host showed the
scene (`debug.gpux.show accessibility`), but gestures injected with `adb shell input` (swipes,
taps, Alt-arrow key combinations) bypass TalkBack's touch exploration: a tap on Activate
activated it directly (clicks 1) and TalkBack never placed its focus. Driving TalkBack needs a
person at the emulator or a device. The emulator's settings were restored afterwards.

### Narrator or NVDA (Windows), Orca (Linux), where available

Not available on this machine.

## Findings

At the owner's request (2026-10-17) the epic fixed findings 1–6 and part of 7; the tables above
record the walk again after the fixes. A further problem the fixed walk found, VoiceOver in
Safari jumping from Details back to the window's start (the mirror rewrote unchanged role and
tabindex attributes on every focus change), was fixed with them. Still open, for the owner's
next walk:

- **Safari: Copies speaks no value.** The spin button's mirror element carries
  `aria-valuenow="1"` and `aria-valuetext="1"`; VoiceOver in Safari says "Copies, stepper"
  (before the value text it said "0%"). Whether WebKit needs the value as text content or
  another attribute is not established. On the macOS desktop it says "1, Copies, stepper".
- **Safari: finding 7 in part.** The IME textarea is now `aria-hidden` while it stands for no
  field, so VoiceOver no longer navigates onto it, but VoiceOver still announces it once when
  it starts on the page before activation ("text entry area, blank, has keyboard focus"),
  because it holds keyboard focus.

VoiceOver, macOS desktop (the `accesskit_macos` 0.27 adapter):

1. **Scores is announced as an empty table.** Expected its two rows and cells with positions.
2. **Details announces no expanded state.** Expected "collapsed", then "expanded".
3. **Copies (a spin button, value 1 of 1–9) is announced as "0%".** Expected its value.
4. **Delete (disabled) is announced as "no action available"**, not as dimmed/unavailable.
5. **Volume is a "circular slider"**, and one VoiceOver increment jumps it from 5 to its maximum
   10, where the step is 1. The scene gives the slider no orientation.

VoiceOver, Safari (the web mirror):

6. **VoiceOver cannot move past the first text field.** On "Title" every VO-Right announces the
   field again, so the controls after it are unreachable by VoiceOver's navigation.
7. **Before activation, VoiceOver lands on an unnamed "text entry area, blank"** (gpux's
   input-method textarea, which the page focuses) and VO-Right does not move past it to the
   page's content.

Not checked here, and still owed by a person: VoiceOver in Chrome, VoiceOver on an iOS device,
TalkBack, Narrator/NVDA and Orca; what VoiceOver says while typing; spoken output itself (only
the caption panel's text was read).
