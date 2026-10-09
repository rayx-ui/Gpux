//! Layout retention's keys and node lifecycle: an element finds the Taffy
//! node it had last frame by its path, id or list index, so inserting,
//! removing or reordering keyed siblings writes nothing else; nodes go when
//! their element does, list rows linger, and the node store stays bounded.
//! With layout retention off, the ordinal pool is unchanged.

use rand::{Rng as _, SeedableRng as _, rngs::StdRng};

use super::oracle_support::oracle_app;
use crate::{
    AppContext as _, Context, ElementId, FastMode, FastStats, Render, ScrollStrategy,
    UniformListScrollHandle, Window, WindowHandle, div, prelude::*, px, uniform_list,
};

const LAYOUT: FastMode = FastMode {
    layout_retention: true,
    ..FastMode::OFF
};

/// A column of children, identified or not, each a sized box with a nested
/// box, and an optional uniform list.
struct Column {
    /// Each child's id and height.
    children: Vec<(usize, f32)>,
    identified: bool,
    /// The same id given to two of the children.
    duplicate: bool,
    list: Option<(usize, UniformListScrollHandle)>,
}

impl Column {
    fn new(count: usize, identified: bool) -> Self {
        Self {
            children: (0..count).map(|id| (id, 10. + id as f32)).collect(),
            identified,
            duplicate: false,
            list: None,
        }
    }
}

impl Render for Column {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let children = self.children.iter().enumerate().map(|(ix, &(id, height))| {
            let child = div()
                .w(px(20.))
                .h(px(height))
                .child(div().size(px(4.)).ml(px(id as f32)));
            if self.duplicate && ix < 2 {
                child.id("twin").into_any_element()
            } else if self.identified {
                child.id(("child", id)).into_any_element()
            } else {
                child.into_any_element()
            }
        });
        div().flex().flex_col().children(children).when_some(
            self.list.clone(),
            |this, (rows, handle)| {
                this.child(
                    uniform_list("rows", rows, |range, _, _| {
                        range
                            .map(|row| {
                                div()
                                    .h(px(10.))
                                    .child(div().w(px(row as f32 % 30. + 1.)).h(px(4.)))
                            })
                            .collect()
                    })
                    .track_scroll(&handle)
                    .h(px(50.)),
                )
            },
        )
    }
}

struct Harness {
    cx: crate::TestAppContext,
    window: WindowHandle<Column>,
}

impl Harness {
    fn open(mode: FastMode, column: Column) -> Self {
        let mut cx = oracle_app(0);
        cx.set_draw_on_flush(false);
        let window = cx.add_window(move |_, _| column);
        cx.update_window(window.into(), |_, window, _| window.set_fast_mode(mode))
            .unwrap();
        let mut harness = Self { cx, window };
        harness.draw();
        harness.draw();
        harness
    }

    fn update(&mut self, update: impl FnOnce(&mut Column)) {
        self.cx
            .update_window(self.window.into(), |root, _, cx| {
                root.downcast::<Column>().unwrap().update(cx, |column, cx| {
                    update(column);
                    cx.notify();
                });
            })
            .unwrap();
    }

    fn draw(&mut self) -> FastStats {
        self.cx
            .update_window(self.window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                window.fast_frame_stats()
            })
            .unwrap()
    }

    fn owned(&mut self) -> crate::FastOwnedState {
        self.cx
            .update_window(self.window.into(), |_, window, _| window.fast_owned_state())
            .unwrap()
    }

    fn frame(&mut self) -> Vec<String> {
        self.cx
            .update_window(self.window.into(), |_, window, _| {
                window.describe_rendered_frame()
            })
            .unwrap()
    }
}

/// The writes one frame made beyond those a frame changing nothing makes
/// (the window root, stretched to the window, is rewritten every frame).
fn writes(frame: &FastStats, steady: &FastStats) -> (u64, u64) {
    (
        frame.style_writes - steady.style_writes,
        frame.children_writes - steady.children_writes,
    )
}

/// A key is the hash of the path from the root: an identified element's
/// key does not depend on the siblings before it, an unidentified one's
/// does, a key never matches the same step under another parent, list items
/// are keyed by index, and what an element lays out while it is prepainted
/// is keyed apart from its children.
#[test]
fn keys_follow_ids_positions_parents_and_list_indices() {
    use crate::fast::layout_key::{
        enter_prepaint_scope, exit_prepaint_scope, layout_key, pop_layout_key, push_layout_key,
    };

    let mut cx = oracle_app(0);
    let window = cx.add_window(|_, _| Column::new(0, true));
    cx.update_window(window.into(), |_, window, _| {
        let id = |name: &'static str| ElementId::Name(name.into());
        // Keys the children of one parent, `before` unidentified siblings
        // coming first.
        let keyed = |window: &mut Window, before: usize, child: Option<&ElementId>| {
            window.fast_layout.begin_frame(true);
            push_layout_key(window, Some(&id("parent")));
            for _ in 0..before {
                push_layout_key(window, None);
                pop_layout_key(window);
            }
            push_layout_key(window, child);
            let key = layout_key(window);
            pop_layout_key(window);
            pop_layout_key(window);
            key.unwrap()
        };
        assert_eq!(
            keyed(window, 0, Some(&id("a"))),
            keyed(window, 3, Some(&id("a"))),
            "an identified element's key moved with the siblings before it"
        );
        assert_ne!(keyed(window, 0, None), keyed(window, 1, None));
        assert_ne!(
            keyed(window, 0, Some(&id("a"))),
            keyed(window, 0, Some(&id("b")))
        );
        // An id equal to a position's index is still another key.
        assert_ne!(
            keyed(window, 0, None),
            keyed(window, 0, Some(&ElementId::Integer(0)))
        );

        // The same step under another parent is another key.
        window.fast_layout.begin_frame(true);
        push_layout_key(window, Some(&id("other parent")));
        push_layout_key(window, Some(&id("a")));
        let elsewhere = layout_key(window).unwrap();
        pop_layout_key(window);
        pop_layout_key(window);
        assert_ne!(elsewhere, keyed(window, 0, Some(&id("a"))));

        // Laid out while its parent is prepainted, an element is keyed under
        // the parent, apart from the parent's children.
        window.fast_layout.begin_frame(true);
        let parent = push_layout_key(window, Some(&id("parent")));
        push_layout_key(window, None);
        let child = layout_key(window).unwrap();
        pop_layout_key(window);
        pop_layout_key(window);
        let scope = enter_prepaint_scope(window, parent);
        push_layout_key(window, None);
        let prepainted = layout_key(window).unwrap();
        pop_layout_key(window);
        exit_prepaint_scope(window, scope);
        assert_ne!(prepainted, child);

        // Off, nothing is keyed.
        window.fast_layout.begin_frame(false);
        push_layout_key(window, Some(&id("parent")));
        assert_eq!(layout_key(window), None);
        pop_layout_key(window);
    })
    .unwrap();
}

/// Inserting, removing and reordering identified siblings writes only the
/// node inserted and the parent's children: every other node is the one it
/// was, unwritten, so Taffy keeps its cache.
#[test]
fn keyed_siblings_move_without_writing_the_others() {
    let mut harness = Harness::open(LAYOUT, Column::new(12, true));
    let steady = harness.draw();
    let kept = harness.owned().kept_layout_nodes;
    assert!(steady.nodes_claimed as usize >= kept, "{steady:?}");
    assert_eq!(steady.nodes_released, 0, "{steady:?}");

    harness.update(|column| column.children.insert(0, (100, 30.)));
    let inserted = harness.draw();
    // The new child and its box are made; the column's children change.
    assert_eq!(writes(&inserted, &steady), (2, 2), "{inserted:?}");
    assert_eq!(harness.owned().kept_layout_nodes, kept + 2);

    harness.update(|column| {
        column.children.remove(5);
    });
    let removed = harness.draw();
    assert_eq!(writes(&removed, &steady), (0, 1), "{removed:?}");
    assert_eq!(removed.nodes_released, 2, "{removed:?}");
    assert_eq!(harness.owned().kept_layout_nodes, kept);

    harness.update(|column| column.children.reverse());
    let reordered = harness.draw();
    assert_eq!(writes(&reordered, &steady), (0, 1), "{reordered:?}");
    assert_eq!(reordered.nodes_released, 0);

    // The frames match the same frames laid out by the pool.
    let mut pooled = Harness::open(FastMode::OFF, Column::new(12, true));
    pooled.update(|column| {
        column.children.insert(0, (100, 30.));
        column.children.remove(5);
        column.children.reverse();
    });
    pooled.draw();
    assert_eq!(harness.frame(), pooled.frame());
}

/// With layout retention off, the ordinal pool runs as before: a child
/// inserted first hands every later node the request of the one before it,
/// and nothing is claimed, kept or released.
#[test]
fn the_off_path_keeps_the_ordinal_pool() {
    let mut harness = Harness::open(FastMode::OFF, Column::new(12, true));
    let steady = harness.draw();
    let nodes = harness.owned().layout_engine_nodes;
    harness.update(|column| column.children.insert(0, (100, 30.)));
    let inserted = harness.draw();
    let (styles, _) = writes(&inserted, &steady);
    assert!(styles > 2, "the pool wrote only {styles} styles");
    assert_eq!(inserted.mechanism_total(), 0, "{inserted:?}");
    let owned = harness.owned();
    assert_eq!(owned.kept_layout_nodes, 0);
    assert_eq!(owned.layout_engine_nodes, nodes + 2);
}

/// Unidentified children are matched by position: changing one leaves the
/// others' nodes alone, and the frame matches the pool's.
#[test]
fn unidentified_children_are_keyed_by_position() {
    let mut harness = Harness::open(LAYOUT, Column::new(8, false));
    let steady = harness.draw();
    harness.update(|column| column.children[3].1 = 50.);
    let changed = harness.draw();
    assert_eq!(writes(&changed, &steady), (1, 0), "{changed:?}");
    assert_eq!(changed.nodes_released, 0);
}

/// Two elements with one key: the second gets a node of its own for the
/// frame, released when the frame ends, and both are laid out right.
#[test]
fn a_duplicate_key_gets_a_transient_node() {
    let mut column = Column::new(6, true);
    column.duplicate = true;
    let mut harness = Harness::open(LAYOUT, column);
    let kept = harness.owned().kept_layout_nodes;
    let frame = harness.draw();
    // The second twin and its box, whose path repeats the first's: made and
    // released every frame.
    assert_eq!(frame.nodes_released, 2, "{frame:?}");
    assert_eq!(harness.owned().kept_layout_nodes, kept);

    let mut column = Column::new(6, true);
    column.duplicate = true;
    let mut pooled = Harness::open(FastMode::OFF, column);
    assert_eq!(harness.frame(), pooled.frame());
}

/// A uniform list lays its rows out while it is prepainted; keyed by index
/// under the list, a steady frame claims every row's nodes again, and rows
/// scrolled out linger for 240 frames before they are released.
#[test]
fn list_rows_are_keyed_by_index_and_linger() {
    let handle = UniformListScrollHandle::new();
    let mut column = Column::new(2, true);
    column.list = Some((100, handle.clone()));
    let mut harness = Harness::open(LAYOUT, column);
    let steady = harness.draw();
    assert_eq!(steady.nodes_released, 0, "{steady:?}");
    assert_eq!(steady.nodes_lingering, 0, "{steady:?}");
    let kept = harness.owned().kept_layout_nodes;

    handle.scroll_to_item(60, ScrollStrategy::Top);
    harness.update(|_| {});
    let scrolled = harness.draw();
    assert!(scrolled.nodes_lingering > 0, "{scrolled:?}");
    assert_eq!(scrolled.nodes_released, 0, "{scrolled:?}");
    let lingering = harness.owned().kept_layout_nodes;
    assert!(lingering > kept);

    // Scrolled back within the linger, the rows find their nodes.
    handle.scroll_to_item(0, ScrollStrategy::Top);
    harness.update(|_| {});
    let back = harness.draw();
    assert_eq!(writes(&back, &steady).0, 0, "{back:?}");

    // Left out for the whole linger, they are released.
    handle.scroll_to_item(60, ScrollStrategy::Top);
    harness.update(|_| {});
    let mut released = 0;
    for _ in 0..crate::fast::layout::LIST_ROW_NODE_LINGER_FRAMES + 1 {
        harness.update(|_| {});
        released += harness.draw().nodes_released;
    }
    assert!(released > 0);
    assert!(harness.owned().kept_layout_nodes <= lingering);
}

/// 1,000 frames of inserts, removes and scrolls: every node the engine
/// holds is kept by key, and they never exceed the largest tree plus every
/// list row lingering.
#[test]
fn the_node_store_stays_bounded_under_churn() {
    let handle = UniformListScrollHandle::new();
    let mut column = Column::new(10, true);
    column.list = Some((100, handle.clone()));
    let mut harness = Harness::open(LAYOUT, column);
    let mut rng = StdRng::seed_from_u64(7);
    let mut next_id = 1000;
    // At most 20 children of 2 nodes, 100 rows of 2 lingering, the list and
    // the column with a few nodes of their own.
    let bound = 20 * 2 + 100 * 2 + 10;
    for frame in 0..1_000 {
        match rng.random_range(0..3) {
            0 => harness.update(|column| {
                if column.children.len() < 20 {
                    let at = rng.random_range(0..=column.children.len());
                    column
                        .children
                        .insert(at, (next_id, rng.random_range(5.0..30.0)));
                    next_id += 1;
                }
            }),
            1 => harness.update(|column| {
                if !column.children.is_empty() {
                    let at = rng.random_range(0..column.children.len());
                    column.children.remove(at);
                }
            }),
            _ => {
                handle.scroll_to_item(rng.random_range(0..100), ScrollStrategy::Top);
                harness.update(|_| {});
            }
        }
        harness.draw();
        let owned = harness.owned();
        assert_eq!(
            owned.layout_engine_nodes, owned.kept_layout_nodes,
            "frame {frame} left a node no key keeps"
        );
        assert!(
            owned.kept_layout_nodes <= bound,
            "frame {frame} kept {} nodes",
            owned.kept_layout_nodes
        );
    }
}

/// Uniform-list rows have no ids, and are keyed by their index under the
/// list: scrolled by one row, every row still in view finds its nodes, and
/// only the row scrolled in is made.
#[test]
fn list_items_without_ids_keep_their_nodes_by_index() {
    let handle = UniformListScrollHandle::new();
    let mut column = Column::new(0, true);
    column.list = Some((100, handle.clone()));
    let mut harness = Harness::open(LAYOUT, column);
    let steady = harness.draw();
    // Five 10px rows show in the 50px list: bringing the sixth to the
    // bottom scrolls by one row.
    handle.scroll_to_item(5, ScrollStrategy::Bottom);
    harness.update(|_| {});
    let scrolled = harness.draw();
    // The row scrolled in: a row and its bar.
    assert_eq!(writes(&scrolled, &steady), (2, 1), "{scrolled:?}");
    assert!(scrolled.nodes_lingering > 0, "{scrolled:?}");
}

/// A node its element no longer requests is released when the frame is
/// cleared, and the engine holds one node fewer.
#[test]
fn a_node_left_unclaimed_for_a_frame_is_released() {
    let mut harness = Harness::open(LAYOUT, Column::new(4, true));
    let before = harness.owned();
    harness.update(|column| column.children.truncate(3));
    let frame = harness.draw();
    assert_eq!(frame.nodes_released, 2, "{frame:?}");
    let after = harness.owned();
    assert_eq!(after.kept_layout_nodes, before.kept_layout_nodes - 2);
    assert_eq!(after.layout_engine_nodes, before.layout_engine_nodes - 2);
}
