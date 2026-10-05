## Diagnostics

- Process 1: 350/350 passed.
- Process 2: 349/350 passed; the only failure was `browser.newContext: Target page, context or browser has been closed` in shell DPR 2.
- Process 3: 348/350 passed; both failures were `page.screenshot: Target page, context or browser has been closed` in TextArea and text-quality DPR 2.
- Failure-only fixture traces showed normal threaded-WASM startup and successful BrowserWebGpu initialization before each closure. They contained no page error, renderer error, or failed visual assertion.
- Fresh narrow replacements for shell, TextArea, and text-quality DPR 2 each passed 35/35.

The closures are therefore treated as long-run Chromium process churn rather than a RayX fixture regression. The replacement protocol preserves page isolation and avoids accepting a partial replicate.
