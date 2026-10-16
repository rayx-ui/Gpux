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
| Date | _to fill_ |
| gpux commit | _to fill_ |
| Machine and OS versions | _to fill_ |

## Expected tree

| Control | Role | Name | State or value |
|---|---|---|---|
| Status line | status (polite live region) | its text | `clicks 0 subscribed false volume …`, then each change announced |
| Activate | button | Activate | |
| Subscribe | check box | Subscribe | unchecked, then checked after a toggle |
| Volume | slider | Volume | its value, then the adjusted value |
| Title | text field | Title | empty, then the typed text |
| Email | text field | Email | required; described by "We never share it" |
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
| Status line | | |
| Activate | | |
| Subscribe | | |
| Volume | | |
| Title | | |
| Email | | |
| Fruits | | |
| Details, Copies, Delete, Code, Scores, Pin | | |
| Activate the button (VO-Space) | | |
| Toggle Subscribe | | |
| Adjust Volume (VO-Up/Down) | | |
| Type into Title | | |

### VoiceOver, Safari (web host)

_Same steps; also note whether the "Enable accessibility" button was found and announced._

### VoiceOver, Chrome (web host)

_Same steps._

### VoiceOver, iOS Simulator or device

_Same steps with swipes and double-taps; adjust the slider with swipe up/down._

### TalkBack, Android emulator or device

_Same steps with swipes and double-taps; adjust the slider with the volume keys or swipe up/down._

### Narrator or NVDA (Windows), Orca (Linux), where available

_Same steps._

## Findings

_Anything missing or wrong, per screen reader, with the control and what was expected._
