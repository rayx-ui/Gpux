# gpux

`gpux` is the RayX-maintained fork of [GPUI](https://gpui.rs), Zed's GPU-accelerated UI framework for Rust.

The repository name and main Rust crate name are both `gpux`. Consumers import `gpux::{...}` while this repository carries the RayX-specific GPUI patch stack separately from the full Zed application workspace.

## What This Repository Contains

`gpux` is intentionally limited to Zed's GPUI crate closure:

- `crates/gpui`, the upstream closure path that contains the main `gpux` framework crate
- platform crates such as `gpui_platform`, `gpui_windows`, `gpui_linux`, `gpui_macos`, `gpui_web`, and `gpui_wgpu`
- GPUI support crates such as `collections`, `scheduler`, `sum_tree`, `util`, `http_client`, `media`, and tracing helpers
- local maintenance and performance tooling needed by the reduced workspace

This repository does not contain the full Zed editor application. Keeping the original GPUI crate boundaries makes upstream imports easier to review: import the GPUI closure from Zed, replay or preserve RayX GPUI patches, then validate the reduced workspace.

## Documentation

- [Installation and usage](https://github.com/ray-platform/gpux/wiki/Installation-and-usage)
- [GPUX Skill](https://github.com/ray-platform/gpux/wiki/GPUX-Skill)

## Who Should Use It

Use `gpux` when you need GPUI with RayX's maintained patch surface, especially from the sibling `RayX.GPUI2` repository.

For a general introduction to GPUI concepts, upstream documentation at [gpui.rs](https://gpui.rs) is still a useful conceptual reference. For exact behavior, APIs, features, examples, and upstream-sync maintenance rules, prefer this repository because `gpux` may differ from upstream Zed GPUI.

## License

Licensed under any one of:

- [GPL-3.0-or-later](LICENSE-GPL)
- [AGPL-3.0-or-later](LICENSE-AGPL)
- [Apache-2.0](LICENSE-APACHE)

## Acknowledgements

[Zed Industries](https://github.com/zed-industries/zed) (GPUI framework) - [wgpu](https://wgpu.rs/) - [cosmic-text](https://github.com/pop-os/cosmic-text)
