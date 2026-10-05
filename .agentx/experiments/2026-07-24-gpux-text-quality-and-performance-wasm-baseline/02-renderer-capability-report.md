## Result: accepted

The Windows platform capability now reflects the renderer feature that determines whether the LCD/subpixel pipeline exists. This removes the inconsistent state where GPUI could request subpixel rasterization although the renderer had not negotiated dual-source blending.

The mechanism is intentionally limited to capability truthfulness. It leaves glyph caching, texture formats, shaders, sampling, transfer, and all editable or table behavior untouched.

Focused headed WASM WebGPU validation passed at DPR 1 and DPR 2. The inspected viewport contains the expected sharp light and dark text specimens. Because the change is Windows-specific and the selected test target is WASM-only, this is a no-regression proof rather than a Windows quality claim.
