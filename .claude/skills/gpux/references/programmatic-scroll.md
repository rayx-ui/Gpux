# Programmatic scroll ownership

Use this reference when changing GPUI `ScrollActivity`, `ScrollBehavior`, Div/List/UniformList scroll APIs, or RayX integration with those primitives.

GPUI owns one neutral activity state machine per scroll owner. `ScrollBehavior::Instant` applies a resolved target in one owner update. `ScrollBehavior::Smooth(duration)` uses the shared Programmatic driver; a zero duration normalizes to instant. A new programmatic request retargets the current generation. Direct wheel, touch, thumb, selection, caret, and IME input cancels programmatic work immediately. Touch pans and their fling momentum reach scroll owners as ordinary `ScrollWheelEvent`s from GPUI's touch recognizer, so `ScrollActivity` tracks only `Idle`, `Direct` (a held scrollbar thumb), and `Programmatic`. Geometry changes clamp or re-resolve through the existing owner, and stale scheduled frames must fail their generation check.

Do not add AppBehavior, PlatformInfo, or RayX component dependencies to gpux. The RayX render/layout owner resolves effective motion and local/app scrolling policy before calling GPUI. GPUI primitives remain usable by non-RayX applications.

Preserve these properties during upstream updates:

- Div, `ListState`, and `UniformListScrollHandle` keep their existing geometry and activity ownership.
- Programmatic scrolling does not create another animation service, and touch momentum stays in GPUI's touch recognizer.
- Direct input always wins and nested propagation retains the existing clamped-offset contract.
- A scrollbar thumb press begins direct activity on its owning `ScrollHandle`, every scrollbar-originated offset remains `Direct` while the thumb is held, and release settles that owner to `Idle`. Do not let each intermediate thumb offset stop activity or header snap policy will fight the gesture.
- Publish concrete `ScrollHandle` thumb offsets in the pointer-input turn against the last accepted viewport geometry, and invalidate the owner plus unique dependents once. Do not queue a preview offset and then schedule a second post-prepaint owner frame. Keep the default queued path for handle implementations that do not own canonical GPUI viewport geometry.
- At most one programmatic frame chain is active; completion, cancellation, and reduced-motion settlement schedule no further frames.
- `SliverHeader` is a RayX policy consumer of this same driver. It resolves app behavior before calling `ScrollHandle::scroll_to`; never accept a header-local timer, duration, easing function, or second activity state machine in gpux or RayX.

Validate with the focused gpux programmatic-scroll tests (`cargo test -p gpux <filter>`) and the `scroll-programmatic` conformance scene (`cargo run -p gpux-testkit -- lane a c --scene scroll-programmatic`); its exact eased offsets need the executor clock, so it runs only in lanes A and C. When visible thumb or wheel behavior changes, also run `scroll-scrollbar` and `scroll-wheel` in lanes B and D (`lane b d --scene <id>`). A change to an API RayX's owners use ships with the matching RayX change and its owner tests in `../RayX`.
