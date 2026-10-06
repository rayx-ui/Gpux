# Upstream: touch sample timestamps

gpux times touch gestures by the platform's sample times. Upstream GPUI (Zed, as of `d544470d1d`)
stamps every `TouchEvent` with `Instant::now()` when `TouchGestureRecognizer::handle_event`
processes it, so fling velocity and the stopped-finger check measure processing delays as well as
finger motion. This file tracks the upstream report and how to return to upstream's recognizer.

| | |
|---|---|
| Upstream issue | Not filed yet; the text is below |
| gpux change | `TouchEvent::timestamp`; `crates/gpui/src/gestures.rs` times recognition by `event.timestamp.unwrap_or(now)` |
| Platforms | gpui_web stamps `Event.timeStamp`, Android NativeActivity stamps `AMotionEvent_getEventTime`, the embedded Android host stamps on arrival, iOS reports none yet |
| Tests | `cargo test -p gpux --lib touch_timestamps` |

When upstream GPUI ships sample times for touch events, take its field and recognizer, keep any
platform stamping it lacks, and delete this file.

## Issue text

Title: `gpui: touch velocity uses processing time, so a slow frame before lift-off suppresses a fling`

### Summary

`TouchGestureRecognizer` stamps each touch sample with `Instant::now()` when it processes the
event (`handle_event` calls `handle_event_at(event, Instant::now())`). Velocity estimation, the
stopped-finger check, multi-tap intervals, and long-press deadlines therefore measure when events
were processed, not when the finger produced them.

### Effect

- A frame that takes longer than `VELOCITY_ASSUME_STOPPED_GAP` (40 ms) just before the finger lifts
  delays the release. The recognizer sees more than 40 ms since the last movement sample, treats
  the finger as stopped, and starts no fling, although the finger was moving fast at lift-off.
- Samples that queue behind a slow frame are processed back to back with nearly identical times.
  The velocity fit then sees the whole displacement over almost no time, or no usable fit at all,
  so the release velocity is wrong or zero.
- Tap intervals and long-press deadlines drift by the same delay.

Browsers align pointer events to animation frames and Android batches motion samples per frame,
so any slow frame during a gesture (for example while content loads) shifts processing time away
from sample time.

### Proposal

Carry the platform's sample time on `TouchEvent` as `timestamp: Option<Instant>`, and stamp it in
the platform layers:

- Web: `Event.timeStamp` shares the `performance.now()` timeline that `web_time::Instant` reads,
  so `Instant::now() - (performance.now() - event.timeStamp)` converts it.
- Android: `AMotionEvent_getEventTime` uses `System.nanoTime()` (`CLOCK_MONOTONIC`), which is also
  what `std::time::Instant` reads there, so the stamp converts by its age.
- iOS: `UITouch.timestamp` is system uptime, which converts the same way.

The recognizer uses `event.timestamp.unwrap_or(now)` for velocity samples, the stopped-finger
check, tap intervals, and long-press deadlines, so platforms without sample times keep today's
behavior. Momentum animation stays on processing time, so a fling starts from wherever the content
is when it can first move.

### Reproduction

A unit test against `TouchGestureRecognizer::handle_event_at`: five moves 16 ms apart, then a
release sampled 16 ms after the last move but processed 120 ms later. Today no momentum starts;
timed by sample, the release flings at the finger's speed. A second test processes the whole
gesture at one instant (a stalled frame): today the velocity fit has no time span, while sample
times recover the finger's 1250 px/s.
