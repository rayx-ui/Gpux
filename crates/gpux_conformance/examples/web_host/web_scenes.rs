//! Browser-only conformance scenes: gpui_web areas no other lane has (DOM overlays, HTML islands,
//! fetched assets, CSS cursors, page lifecycle, safe areas, the visitor's shortcut modifier). They live with the browser host
//! because the shared catalog runs every scene in lane A. Their scripts follow the catalog's
//! conventions, so the Playwright driver runs them like any lane-B scene.

use gpux::{
    AnyElement, AnyView, App, AppContext as _, Context, DispatchPhase, ElementId, FocusHandle,
    HtmlElementContentFit, HtmlElementInputMode, HtmlElementLifetime, HtmlElementWheelBehavior,
    HtmlIslandInputMode, IntoElement, KeyBinding, KeyContext, Keystroke, ListAlignment, ListState,
    MouseExitEvent, Render, ScrollHandle, Subscription, Window, actions, anchored, canvas,
    deferred, div, list, point, prelude::*, px, rgb, svg,
};
use gpux_conformance::{
    Expect, Lane, SceneSpec, Step,
    scenes::common::{
        BLUE, BLUE_HEX, GRAY_HEX, GREEN_HEX, WHITE, id, root, static_view, status, status_is,
    },
    scroll_layers::{ScrollStatus, card_row},
};

/// Every browser-only scene.
pub static WEB_SCENES: &[SceneSpec] = &[
    HTML_ELEMENT,
    COMPOSITION,
    HTML_ISLAND,
    SVG_ASSET,
    CURSORS,
    LIFECYCLE,
    SAFE_AREA,
    SHORTCUT_MODIFIER,
    SCROLL_LAYERS,
    SCROLL_LAYERS_HTML,
    SCROLL_LAYERS_ISLAND,
];

/// The browser-only scene with this id.
pub fn scene(id: &str) -> Option<&'static SceneSpec> {
    WEB_SCENES.iter().find(|scene| scene.id == id)
}

/// The SVG `web-svg-asset` fetches; `gpux-testkit web build` serves it from the build.
pub const SVG_ASSET_PATH: &str = "/assets/conformance.svg";

/// A DOM button the `web-html-element` overlay shows. It counts its own (DOM) clicks.
const DOM_BUTTON: &str = r#"<button id="gpux-dom-button" style="background:#0000ff;color:#ffffff;border:0;border-radius:0;margin:0;padding:0;font:16px sans-serif" onclick="this.dataset.clicks = String(Number(this.dataset.clicks || 0) + 1)">DOM</button>"#;

/// A trusted DOM overlay toggled by a gpui button.
struct DomOverlay {
    shown: bool,
}

impl Render for DomOverlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(status(format!("dom shown {}", self.shown)))
            .child(
                div()
                    .id("toggle")
                    .absolute()
                    .top(px(30.))
                    .left(px(20.))
                    .w(px(60.))
                    .h(px(24.))
                    .bg(rgb(GREEN_HEX))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.shown = !this.shown;
                        cx.notify();
                    })),
            )
            .when(self.shown, |this| {
                this.child(
                    div()
                        .id("dom-host")
                        // Its `accessibility_id` names the overlay, so the screen-reader mirror
                        // reads the overlay's DOM here (`aria-owns`).
                        .role(gpux::Role::Group)
                        .aria_label("DOM content")
                        .accessibility_id("gpux-dom-button")
                        .absolute()
                        .top(px(70.))
                        .left(px(20.))
                        .w(px(160.))
                        .h(px(50.))
                        .child(
                            canvas(
                                |_, _, _| {},
                                |bounds, _, window, _| {
                                    window.paint_html_element(
                                        ElementId::from("gpux-dom-button"),
                                        bounds,
                                        HtmlElementInputMode::DomOwned,
                                        HtmlElementLifetime::PruneWhenAbsent,
                                        HtmlElementContentFit::Fill,
                                        HtmlElementWheelBehavior::Browser,
                                        1,
                                        DOM_BUTTON.into(),
                                        "".into(),
                                    );
                                },
                            )
                            .size_full(),
                        ),
                )
            })
    }
}

pub const HTML_ELEMENT: SceneSpec = SceneSpec {
    id: "web-html-element",
    areas: &[],
    size: (240., 140.),
    scales: &[1., 2.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| cx.new(|_| DomOverlay { shown: true }).into(),
    script: || {
        vec![
            Step::Expect(Expect::Exists(status_is("dom shown true"))),
            // The DOM overlay paints above the canvas where the element is laid out.
            Step::probe(24., 74., BLUE),
            Step::Click(id("toggle")),
            Step::Expect(Expect::Exists(status_is("dom shown false"))),
            Step::probe(24., 74., WHITE),
            Step::Click(id("toggle")),
            Step::Expect(Expect::Exists(status_is("dom shown true"))),
            Step::probe(24., 74., BLUE),
        ]
    },
};

/// A blue DOM block the `web-composition` overlay shows. It counts its own (DOM) clicks.
const DOM_BLOCK: &str = r#"<div id="gpux-dom-block" style="width:100%;height:100%;background:#0000ff" onclick="this.dataset.clicks = String(Number(this.dataset.clicks || 0) + 1)"></div>"#;

/// A DOM element under a GPUI popover, in a window that composes when it can.
struct ComposedPage {
    composition: &'static str,
    popover_clicks: usize,
    /// The pointer exits GPUI saw: moving between the canvas and an overlay's input
    /// shield must not count as leaving the window.
    exits: usize,
}

impl Render for ComposedPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        root()
            .child(status(format!(
                "composition {} clicks {}",
                self.composition, self.popover_clicks
            )))
            .child(status(format!("exits {}", self.exits)))
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let page = page.clone();
                        window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                            if phase == DispatchPhase::Bubble {
                                page.update(cx, |page, cx| {
                                    page.exits += 1;
                                    cx.notify();
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
            .child(
                div()
                    .id("dom-host")
                    .absolute()
                    .top(px(40.))
                    .left(px(20.))
                    .w(px(160.))
                    .h(px(80.))
                    .child(
                        canvas(
                            |_, _, _| {},
                            |bounds, _, window, _| {
                                window.paint_html_element(
                                    ElementId::from("gpux-dom-block"),
                                    bounds,
                                    HtmlElementInputMode::DomOwned,
                                    HtmlElementLifetime::PruneWhenAbsent,
                                    HtmlElementContentFit::Fill,
                                    HtmlElementWheelBehavior::Browser,
                                    1,
                                    DOM_BLOCK.into(),
                                    "".into(),
                                );
                            },
                        )
                        .size_full(),
                    ),
            )
            .child(deferred(
                anchored().position(point(px(60.), px(60.))).child(
                    div()
                        .id("popover")
                        .w(px(80.))
                        .h(px(40.))
                        .bg(rgb(GREEN_HEX))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.popover_clicks += 1;
                            cx.notify();
                        })),
                ),
            ))
    }
}

pub const COMPOSITION: SceneSpec = SceneSpec {
    id: "web-composition",
    areas: &[],
    size: (240., 160.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |window, cx| {
        let composition = match window.enable_window_composition() {
            Ok(_) => "on",
            Err(_) => "unsupported",
        };
        cx.new(|_| ComposedPage {
            composition,
            popover_clicks: 0,
            exits: 0,
        })
        .into()
    },
    // The composition spec reads the page's pixels and drives its input.
    script: || vec![Step::Expect(Expect::Exists(id("popover")))],
};

/// The trusted HTML an island shows: a blue block.
const ISLAND_HTML: &str =
    r#"<div id="gpux-island-block" style="width:100%;height:100%;background:#0000ff"></div>"#;

fn html_island() -> AnyElement {
    root()
        .child(status("island"))
        .child(
            div()
                .id("island-host")
                .absolute()
                .top(px(40.))
                .left(px(20.))
                .w(px(160.))
                .h(px(50.))
                .child(
                    canvas(
                        |_, _, _| {},
                        |bounds, _, window, _| {
                            window.paint_html_island(
                                ElementId::from("gpux-island"),
                                bounds,
                                HtmlIslandInputMode::GpuiOwned,
                                1,
                                ISLAND_HTML.into(),
                            );
                        },
                    )
                    .size_full(),
                ),
        )
        .into_any_element()
}

pub const HTML_ISLAND: SceneSpec = SceneSpec {
    id: "web-html-island",
    areas: &[],
    size: (240., 120.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| static_view(cx, html_island),
    // Island pixels depend on the browser's HTML-in-canvas support; the island spec probes them
    // where the browser has it.
    script: || vec![Step::Expect(Expect::Exists(id("island-host")))],
};

fn svg_asset() -> AnyElement {
    root()
        .child(status("svg asset"))
        .child(
            div()
                .id("asset")
                .absolute()
                .top(px(40.))
                .left(px(20.))
                .size(px(40.))
                .child(
                    svg()
                        .external_path(SVG_ASSET_PATH)
                        .size_full()
                        .text_color(rgb(BLUE_HEX)),
                ),
        )
        .into_any_element()
}

pub const SVG_ASSET: SceneSpec = SceneSpec {
    id: "web-svg-asset",
    areas: &[],
    size: (120., 100.),
    scales: &[1., 2.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| static_view(cx, svg_asset),
    script: || {
        vec![
            Step::Expect(Expect::Bounds(id("asset"), (20., 40., 40., 40.), 0.01)),
            // The fetched SVG is a full square, tinted by the text color.
            Step::probe(40., 60., BLUE),
            Step::probe(10., 60., WHITE),
        ]
    },
};

fn cursors() -> AnyElement {
    let target = |name: &'static str, left: f32| {
        div()
            .id(name)
            .absolute()
            .top(px(40.))
            .left(px(left))
            .size(px(40.))
            .bg(rgb(GRAY_HEX))
    };
    root()
        .child(status("cursors"))
        .child(target("pointer", 20.).cursor_pointer())
        .child(target("text", 80.).cursor_text())
        .child(target("grab", 140.).cursor_grab())
        .into_any_element()
}

pub const CURSORS: SceneSpec = SceneSpec {
    id: "web-cursors",
    areas: &[],
    size: (200., 100.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| static_view(cx, cursors),
    script: || {
        vec![
            Step::Expect(Expect::Count(id("pointer"), 1)),
            Step::Hover(id("pointer")),
            Step::Hover(id("text")),
            Step::Hover(id("grab")),
            Step::Expect(Expect::Count(id("grab"), 1)),
        ]
    },
};

/// Reports whether the window is active and every activation change it observed.
struct Lifecycle {
    changes: Vec<bool>,
    _activation: Subscription,
}

impl Render for Lifecycle {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let changes: Vec<&str> = self
            .changes
            .iter()
            .map(|active| if *active { "active" } else { "inactive" })
            .collect();
        root().child(status(format!(
            "{} changes [{}]",
            if window.is_window_active() {
                "active"
            } else {
                "inactive"
            },
            changes.join(" ")
        )))
    }
}

fn lifecycle(window: &mut Window, cx: &mut App) -> AnyView {
    cx.new(|cx| Lifecycle {
        changes: Vec::new(),
        _activation: cx.observe_window_activation(window, |this: &mut Lifecycle, window, cx| {
            this.changes.push(window.is_window_active());
            cx.notify();
        }),
    })
    .into()
}

pub const LIFECYCLE: SceneSpec = SceneSpec {
    id: "web-lifecycle",
    areas: &[],
    size: (320., 60.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: lifecycle,
    script: || vec![Step::Expect(Expect::Exists(status_is("active changes []")))],
};

/// Reports the window's fully visible bounds, which exclude the safe-area insets.
struct SafeArea;

impl Render for SafeArea {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let visible = window.fully_visible_bounds();
        root().child(status(format!(
            "visible {} {} {} {}",
            f32::from(visible.origin.x).round(),
            f32::from(visible.origin.y).round(),
            f32::from(visible.size.width).round(),
            f32::from(visible.size.height).round()
        )))
    }
}

pub const SAFE_AREA: SceneSpec = SceneSpec {
    id: "web-safe-area",
    areas: &[],
    size: (320., 60.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| cx.new(|_| SafeArea).into(),
    script: || vec![Step::Expect(Expect::Exists(id("status")))],
};

actions!(web_conformance, [Shortcut, MacShortcut]);

/// Follows the visitor's keyboard conventions: a `secondary-k` binding, a binding for macOS only
/// (`os == macos`), and how `secondary-k` is written and shown. Command is the secondary modifier
/// in Apple browsers and Control elsewhere, whatever the WASM was compiled for.
struct ShortcutModifier {
    focus: FocusHandle,
    shortcuts: usize,
    mac_shortcuts: usize,
}

impl Render for ShortcutModifier {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut context = KeyContext::new_with_defaults();
        context.add("Shortcuts");
        let os = context.get("os").cloned().unwrap_or_default();
        let secondary_k = Keystroke::parse("secondary-k").expect("a valid keystroke");
        root()
            .child(status(format!(
                "os {os} written {} shown {} shortcuts {} mac {}",
                secondary_k.unparse(),
                secondary_k,
                self.shortcuts,
                self.mac_shortcuts
            )))
            .child(
                div()
                    .id("target")
                    .key_context(context)
                    .track_focus(&self.focus)
                    .w(px(100.))
                    .h(px(30.))
                    .bg(rgb(GRAY_HEX))
                    .on_action(cx.listener(|this, _: &Shortcut, _, cx| {
                        this.shortcuts += 1;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &MacShortcut, _, cx| {
                        this.mac_shortcuts += 1;
                        cx.notify();
                    })),
            )
    }
}

fn shortcut_modifier(_: &mut Window, cx: &mut App) -> AnyView {
    cx.bind_keys([
        KeyBinding::new("secondary-k", Shortcut, Some("Shortcuts")),
        KeyBinding::new("f2", MacShortcut, Some("Shortcuts && os == macos")),
    ]);
    cx.new(|cx| ShortcutModifier {
        focus: cx.focus_handle(),
        shortcuts: 0,
        mac_shortcuts: 0,
    })
    .into()
}

pub const SHORTCUT_MODIFIER: SceneSpec = SceneSpec {
    id: "web-shortcut-modifier",
    areas: &[],
    size: (420., 70.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: shortcut_modifier,
    script: || {
        vec![
            Step::Focus(id("target")),
            // The bridge sends `secondary-k` with the visitor's secondary modifier.
            Step::Keys("secondary-k secondary-k".into()),
            Step::Expect(Expect::Exists(id("status"))),
        ]
    },
};

/// A scrolling `div` and a `list` side by side, each of cards with borders and shadows.
struct ScrollLayersPage {
    scroll: ScrollHandle,
    list: ListState,
    status: gpux::Entity<ScrollStatus>,
}

impl Render for ScrollLayersPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root()
            .child(self.status.clone())
            .child(
                div()
                    .id("scroller")
                    .absolute()
                    .top(px(30.))
                    .left(px(10.))
                    .w(px(200.))
                    .h(px(300.))
                    .bg(rgb(0xf4f1ea))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children((0..300).map(card_row)),
            )
            .child(
                div()
                    .id("rows")
                    .absolute()
                    .top(px(30.))
                    .left(px(230.))
                    .w(px(200.))
                    .h(px(300.))
                    .bg(rgb(0xf4f1ea))
                    .child(list(self.list.clone(), |index, _, _| card_row(index)).size_full()),
            )
    }
}

pub const SCROLL_LAYERS: SceneSpec = SceneSpec {
    id: "web-scroll-layers",
    areas: &[],
    size: (440., 340.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| {
        let scroll = ScrollHandle::new();
        let list = ListState::new(300, ListAlignment::Top, px(64.)).measure_all();
        let status = cx.new(|_| ScrollStatus::new(scroll.clone(), Some(list.clone())));
        cx.new(|_| ScrollLayersPage {
            scroll,
            list,
            status,
        })
        .into()
    },
    // The scroll-layers spec drives the wheel and reads the page's pixels and counters.
    script: || vec![Step::Expect(Expect::Exists(id("scroller")))],
};

/// A scrolling `div` that holds a DOM element or an HTML island: its content cannot be tiled.
struct ScrollLayersHtmlPage {
    scroll: ScrollHandle,
    status: gpux::Entity<ScrollStatus>,
    /// Holds an island (`Window::paint_html_island`) rather than an element.
    island: bool,
}

impl Render for ScrollLayersHtmlPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        root().child(self.status.clone()).child(
            div()
                .id("scroller")
                .absolute()
                .top(px(30.))
                .left(px(10.))
                .w(px(200.))
                .h(px(300.))
                .bg(rgb(0xf4f1ea))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child({
                    let island = self.island;
                    div().id("dom-host").h(px(64.)).w(px(160.)).child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                if island {
                                    window.paint_html_island(
                                        ElementId::from("gpux-island"),
                                        bounds,
                                        HtmlIslandInputMode::GpuiOwned,
                                        1,
                                        ISLAND_HTML.into(),
                                    );
                                } else {
                                    window.paint_html_element(
                                        ElementId::from("gpux-dom-button"),
                                        bounds,
                                        HtmlElementInputMode::DomOwned,
                                        HtmlElementLifetime::PruneWhenAbsent,
                                        HtmlElementContentFit::Fill,
                                        HtmlElementWheelBehavior::Browser,
                                        1,
                                        DOM_BUTTON.into(),
                                        "".into(),
                                    );
                                }
                            },
                        )
                        .size_full(),
                    )
                })
                .children((0..200).map(card_row)),
        )
    }
}

pub const SCROLL_LAYERS_HTML: SceneSpec = SceneSpec {
    id: "web-scroll-layers-html",
    areas: &[],
    size: (440., 340.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| {
        let scroll = ScrollHandle::new();
        let status = cx.new(|_| ScrollStatus::new(scroll.clone(), None));
        cx.new(|_| ScrollLayersHtmlPage {
            scroll,
            status,
            island: false,
        })
        .into()
    },
    script: || vec![Step::Expect(Expect::Exists(id("scroller")))],
};

pub const SCROLL_LAYERS_ISLAND: SceneSpec = SceneSpec {
    id: "web-scroll-layers-island",
    areas: &[],
    size: (440., 340.),
    scales: &[1.],
    lanes: &[Lane::B],
    golden: false,
    build: |_, cx| {
        let scroll = ScrollHandle::new();
        let status = cx.new(|_| ScrollStatus::new(scroll.clone(), None));
        cx.new(|_| ScrollLayersHtmlPage {
            scroll,
            status,
            island: true,
        })
        .into()
    },
    script: || vec![Step::Expect(Expect::Exists(id("scroller")))],
};
