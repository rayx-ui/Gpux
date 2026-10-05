## Expected outcome

On adapters without dual-source blending, the Windows platform will report subpixel text unsupported, so the existing GPUI mode selection falls back to grayscale before text rasterization. On capable adapters, the existing subpixel path is preserved.

The WASM Test Suite continues to report grayscale text and passes its light/dark DPR 1/2 image checks. This validates no cross-platform regression under the user-selected target; Windows runtime quality remains explicitly unmeasured in this experiment.
