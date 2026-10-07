use std::cell::Cell;

use wasm_bindgen::{JsCast as _, JsValue};

/// Browser-only bounded counters for the viewport segment of the MediaQuery
/// pipeline. They are scalar-only so enabling the probe cannot retain input or
/// rendered content.
#[derive(Default)]
pub(crate) struct MediaQueryPipelineDiagnostics {
    reset_epoch: Cell<u64>,
    raw_viewport_signals: Cell<u64>,
    animation_frame_requests: Cell<u64>,
    coalesced_flushes: Cell<u64>,
    gpui_resize_emissions: Cell<u64>,
    pipeline_started_at_ms: Cell<Option<f64>>,
    pipeline_samples: Cell<u64>,
    pipeline_total_ms: Cell<f64>,
    pipeline_max_ms: Cell<f64>,
    last_animation_frame_at_ms: Cell<Option<f64>>,
    frame_intervals: Cell<u64>,
    frame_interval_total_ms: Cell<f64>,
    frame_interval_max_ms: Cell<f64>,
    heartbeat_gaps: Cell<u64>,
    long_tasks: Cell<u64>,
}

impl MediaQueryPipelineDiagnostics {
    const VERSION: &'static str = "media-query-pipeline/v1";

    pub(crate) fn raw_viewport_signal(&self) {
        if !self.enabled() {
            return;
        }
        self.raw_viewport_signals
            .set(self.raw_viewport_signals.get().saturating_add(1));
        self.write();
    }

    pub(crate) fn animation_frame_request(&self) {
        if !self.enabled() {
            return;
        }
        self.animation_frame_requests
            .set(self.animation_frame_requests.get().saturating_add(1));
        self.record_animation_frame_interval();
        self.write();
    }

    pub(crate) fn coalesced_flush(&self) {
        if !self.enabled() {
            return;
        }
        self.coalesced_flushes
            .set(self.coalesced_flushes.get().saturating_add(1));
        // The raw observer-to-animation-frame wait is scheduler latency, not
        // MediaQuery pipeline work. Measure the coalesced flush through the
        // synchronous GPUI resize emission so the metric remains actionable.
        self.pipeline_started_at_ms.set(Some(monotonic_now_ms()));
        self.write();
    }

    pub(crate) fn gpui_resize_emission(&self) {
        if !self.enabled() {
            return;
        }
        self.gpui_resize_emissions
            .set(self.gpui_resize_emissions.get().saturating_add(1));
        self.finish_pipeline_sample();
        self.write();
    }

    fn enabled(&self) -> bool {
        gpui_wgpu::web_globals::flag("MediaQueryDiagnosticsEnabled")
    }

    fn record_animation_frame_interval(&self) {
        let now = monotonic_now_ms();
        if let Some(previous) = self.last_animation_frame_at_ms.replace(Some(now)) {
            let interval = (now - previous).max(0.0);
            self.frame_intervals
                .set(self.frame_intervals.get().saturating_add(1));
            self.frame_interval_total_ms
                .set(self.frame_interval_total_ms.get() + interval);
            self.frame_interval_max_ms
                .set(self.frame_interval_max_ms.get().max(interval));
            if interval >= 500.0 {
                self.heartbeat_gaps
                    .set(self.heartbeat_gaps.get().saturating_add(1));
            }
            if interval >= 50.0 {
                self.long_tasks.set(self.long_tasks.get().saturating_add(1));
            }
        }
    }

    fn finish_pipeline_sample(&self) {
        let Some(started_at) = self.pipeline_started_at_ms.take() else {
            return;
        };
        let elapsed = (monotonic_now_ms() - started_at).max(0.0);
        self.pipeline_samples
            .set(self.pipeline_samples.get().saturating_add(1));
        self.pipeline_total_ms
            .set(self.pipeline_total_ms.get() + elapsed);
        self.pipeline_max_ms
            .set(self.pipeline_max_ms.get().max(elapsed));
    }

    fn write(&self) {
        if !self.enabled() {
            return;
        }
        let requested_reset_epoch = gpui_wgpu::web_globals::read("MediaQueryDiagnosticsResetEpoch")
            .and_then(|value| value.as_f64())
            .filter(|value| *value >= 0.0)
            .map(|value| value as u64);
        if let Some(epoch) = requested_reset_epoch.filter(|epoch| *epoch > self.reset_epoch.get()) {
            self.reset_epoch.set(epoch);
            self.raw_viewport_signals.set(0);
            self.animation_frame_requests.set(0);
            self.coalesced_flushes.set(0);
            self.gpui_resize_emissions.set(0);
            self.pipeline_started_at_ms.set(None);
            self.pipeline_samples.set(0);
            self.pipeline_total_ms.set(0.0);
            self.pipeline_max_ms.set(0.0);
            self.last_animation_frame_at_ms.set(None);
            self.frame_intervals.set(0);
            self.frame_interval_total_ms.set(0.0);
            self.frame_interval_max_ms.set(0.0);
            self.heartbeat_gaps.set(0);
            self.long_tasks.set(0);
        }
        let target = gpui_wgpu::web_globals::read("MediaQueryPipeline")
            .and_then(|value| value.dyn_into::<js_sys::Object>().ok())
            .unwrap_or_else(js_sys::Object::new);
        let set = |key: &str, value: JsValue| {
            let _ = js_sys::Reflect::set(&target, &JsValue::from_str(key), &value);
        };
        set("diagnosticVersion", JsValue::from_str(Self::VERSION));
        set(
            "resetEpoch",
            JsValue::from_f64(self.reset_epoch.get() as f64),
        );
        set(
            "rawViewportSignals",
            JsValue::from_f64(self.raw_viewport_signals.get() as f64),
        );
        set(
            "animationFrameRequests",
            JsValue::from_f64(self.animation_frame_requests.get() as f64),
        );
        set(
            "coalescedFlushes",
            JsValue::from_f64(self.coalesced_flushes.get() as f64),
        );
        set(
            "gpuiResizeEmissions",
            JsValue::from_f64(self.gpui_resize_emissions.get() as f64),
        );
        let frame_intervals = self.frame_intervals.get();
        set("frameIntervals", JsValue::from_f64(frame_intervals as f64));
        set(
            "frameIntervalMeanMs",
            JsValue::from_f64(if frame_intervals == 0 {
                0.0
            } else {
                self.frame_interval_total_ms.get() / frame_intervals as f64
            }),
        );
        set(
            "frameIntervalMaxMs",
            JsValue::from_f64(self.frame_interval_max_ms.get()),
        );
        let pipeline_samples = self.pipeline_samples.get();
        set(
            "pipelineSamples",
            JsValue::from_f64(pipeline_samples as f64),
        );
        set(
            "pipelineMeanMs",
            JsValue::from_f64(if pipeline_samples == 0 {
                0.0
            } else {
                self.pipeline_total_ms.get() / pipeline_samples as f64
            }),
        );
        set(
            "pipelineMaxMs",
            JsValue::from_f64(self.pipeline_max_ms.get()),
        );
        set(
            "heartbeatGaps",
            JsValue::from_f64(self.heartbeat_gaps.get() as f64),
        );
        set("longTasks", JsValue::from_f64(self.long_tasks.get() as f64));
        for key in [
            "snapshotAttempts",
            "snapshotPublications",
            "changedAspects",
            "dependencyRegistrations",
            "dependencyReleases",
            "consumerRenders",
            "branchTransitions",
        ] {
            let value = JsValue::from_str(key);
            let is_missing = js_sys::Reflect::get(&target, &value)
                .ok()
                .and_then(|value| value.as_f64())
                .is_none();
            if is_missing {
                let _ = js_sys::Reflect::set(&target, &value, &JsValue::from_f64(0.0));
            }
        }
        gpui_wgpu::web_globals::publish("MediaQueryPipeline", &target);
    }
}

fn monotonic_now_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or_else(js_sys::Date::now)
}
