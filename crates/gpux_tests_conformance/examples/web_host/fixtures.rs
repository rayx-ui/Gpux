//! Browser fixtures the lane-B specs call directly, migrated from `gpui_web`'s `hello_web`
//! scripts: a queue shared by the browser main thread and real web workers, and the text
//! system's missing-glyph notifications.

use gpux::{
    FallbackFontClass, FontRun, MissingGlyph, MissingGlyphSink, Priority, PriorityQueueReceiver,
    PriorityQueueSender, font, px,
};
use std::{
    borrow::Cow,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use wasm_bindgen::prelude::*;

/// Exercises both directions of a queue shared by the browser and real workers.
#[wasm_bindgen]
pub struct QueueStress {
    sender: PriorityQueueSender<Option<usize>>,
    receiver: PriorityQueueReceiver<usize>,
    ready: Arc<AtomicUsize>,
    stopped: Arc<AtomicUsize>,
    workers: Vec<wasm_thread::JoinHandle<()>>,
    seen: Vec<bool>,
    sent: usize,
    received: usize,
}

#[wasm_bindgen]
impl QueueStress {
    /// Starts `worker_count` workers that echo every item back.
    #[wasm_bindgen(constructor)]
    pub fn new(worker_count: usize, item_count: usize) -> Self {
        assert!(worker_count > 0 && item_count > 0);
        let (sender, receiver) = PriorityQueueReceiver::<Option<usize>>::new();
        let (reply_sender, reply_receiver) = PriorityQueueReceiver::new();
        let reply_sender = Arc::new(reply_sender);
        let ready = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicUsize::new(0));
        let workers = (0..worker_count)
            .map(|_| {
                let mut receiver = receiver.clone();
                let reply_sender = reply_sender.clone();
                let ready = ready.clone();
                let stopped = stopped.clone();
                wasm_thread::spawn(move || {
                    ready.fetch_add(1, Ordering::Release);
                    while let Some(item) = receiver.pop().expect("receive work") {
                        reply_sender.send(priority(item), item).expect("send reply");
                    }
                    stopped.fetch_add(1, Ordering::Release);
                })
            })
            .collect();
        Self {
            sender,
            receiver: reply_receiver,
            ready,
            stopped,
            workers,
            seen: vec![false; item_count],
            sent: 0,
            received: 0,
        }
    }

    /// Whether every worker started.
    pub fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) == self.workers.len()
    }

    /// Sends the next `count` items from the browser main thread.
    pub fn send_batch(&mut self, count: usize) {
        let end = (self.sent + count).min(self.seen.len());
        for item in self.sent..end {
            self.sender
                .spin_send(priority(item), Some(item))
                .expect("send work from browser main thread");
        }
        self.sent = end;
    }

    /// Receives every reply waiting on the main thread and returns how many arrived so far.
    pub fn drain(&mut self) -> usize {
        while let Some(item) = self
            .receiver
            .spin_try_pop()
            .expect("receive reply on browser main thread")
        {
            let seen = self.seen.get_mut(item).expect("unexpected reply");
            assert!(!*seen, "duplicate reply");
            *seen = true;
            self.received += 1;
        }
        self.received
    }

    /// Checks every item came back once and stops the workers.
    pub fn stop(&mut self) {
        assert_eq!(self.sent, self.seen.len());
        assert_eq!(self.received, self.sent);
        assert!(self.seen.iter().all(|seen| *seen));
        // The queue's disconnect does not wake empty receivers, so each worker gets a stop item.
        for _ in &self.workers {
            self.sender
                .spin_send(Priority::Low, None)
                .expect("stop worker");
        }
    }

    /// Whether every worker stopped.
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire) == self.workers.len()
    }
}

fn priority(item: usize) -> Priority {
    match item % 3 {
        0 => Priority::High,
        1 => Priority::Medium,
        _ => Priority::Low,
    }
}

/// Negative control: a browser-main WASM atomic wait must trap, even with a zero timeout.
#[wasm_bindgen(js_name = forbiddenMainThreadWait)]
pub fn forbidden_main_thread_wait() {
    let value = std::sync::atomic::AtomicI32::new(0);
    // SAFETY: the pointer is to a live, aligned i32; the call is expected to trap.
    unsafe {
        std::arch::wasm32::memory_atomic_wait32(value.as_ptr(), 0, 0);
    }
}

#[derive(Default)]
struct Reports(Mutex<Vec<MissingGlyph>>);

impl MissingGlyphSink for Reports {
    fn report(&self, missing: Vec<MissingGlyph>) {
        self.0.lock().expect("report lock").extend(missing);
    }
}

fn check(holds: bool, problem: impl Into<String>) -> Result<(), JsValue> {
    if holds {
        Ok(())
    } else {
        Err(JsValue::from_str(&problem.into()))
    }
}

/// Lays out text in Lilex and checks which graphemes the text system reports missing: CJK it
/// cannot draw is reported once per grapheme as monospace fallback, emoji resolve to emoji
/// glyphs without reports, bundled glyphs stay in Lilex, and a cleared sink reports nothing.
#[wasm_bindgen(js_name = missingGlyphNotifications)]
pub fn missing_glyph_notifications() -> Result<(), JsValue> {
    let platform = gpui_platform::current_platform(false);
    let text_system = platform.text_system();
    text_system
        .add_fonts(vec![Cow::Borrowed(gpux_fonts::LILEX_REGULAR)])
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let font_id = text_system
        .font_id(&font("Lilex"))
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let reports = Arc::new(Reports::default());
    text_system.set_missing_glyph_sink(Some(reports.clone()));
    let take = || std::mem::take(&mut *reports.0.lock().expect("report lock"));

    let text = "A界😀👨‍👩‍👧‍👦か\u{3099}";
    let runs = [FontRun {
        len: text.len(),
        font_id,
        metrics: Default::default(),
    }];
    let layout = text_system.layout_line(text, px(24.), &runs);
    check(
        layout
            .runs
            .iter()
            .any(|run| run.glyphs.iter().any(|glyph| glyph.is_emoji)),
        "emoji did not resolve to emoji glyphs",
    )?;
    let missing = take();
    let graphemes: Vec<String> = missing
        .iter()
        .map(|report| report.grapheme().to_string())
        .collect();
    check(
        graphemes == ["界", "か\u{3099}"],
        format!("missing graphemes {graphemes:?}, expected [\"界\", \"か\u{3099}\"]"),
    )?;
    check(
        missing
            .iter()
            .all(|report| report.font_class() == FallbackFontClass::Monospace),
        "a missing Lilex grapheme was not reported as monospace fallback",
    )?;

    for graphemes in [
        vec!["🕸", "🕸", "🕸"],
        vec!["🕸", "🕸\u{fe0f}", "🕸\u{fe0e}", "🕸"],
    ] {
        let text = graphemes.concat();
        let layout = text_system.layout_line(
            &text,
            px(24.),
            &[FontRun {
                len: text.len(),
                font_id,
                metrics: Default::default(),
            }],
        );
        let glyphs: Vec<_> = layout
            .runs
            .iter()
            .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.font_id, glyph)))
            .collect();
        check(
            glyphs.len() == graphemes.len(),
            format!(
                "{text}: {} glyphs for {} graphemes",
                glyphs.len(),
                graphemes.len()
            ),
        )?;
        let mut source_index = 0;
        for ((resolved_font, glyph), grapheme) in glyphs.iter().zip(&graphemes) {
            check(
                *resolved_font != font_id,
                format!("{grapheme} did not fall back"),
            )?;
            check(
                glyph.index == source_index,
                format!("{grapheme} lost its source index"),
            )?;
            check(
                glyph.is_emoji == grapheme.ends_with('\u{fe0f}'),
                format!("{grapheme:?} has the wrong emoji presentation"),
            )?;
            source_index += grapheme.len();
        }
        check(
            source_index == text.len(),
            format!("{text}: graphemes were dropped"),
        )?;
        check(
            take().is_empty(),
            "every spider-web glyph must resolve, not just the first cached raster",
        )?;
    }

    let bundled = "Hello ©";
    let layout = text_system.layout_line(
        bundled,
        px(24.),
        &[FontRun {
            len: bundled.len(),
            font_id,
            metrics: Default::default(),
        }],
    );
    for run in &layout.runs {
        check(run.font_id == font_id, "bundled text left Lilex")?;
        check(
            run.glyphs.iter().all(|glyph| !glyph.is_emoji),
            "bundled text became emoji",
        )?;
    }
    check(take().is_empty(), "bundled glyphs were reported missing")?;

    text_system.set_missing_glyph_sink(None);
    text_system.layout_line(text, px(24.), &runs);
    check(take().is_empty(), "a cleared sink still received reports")
}
