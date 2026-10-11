//! The scene's bounds tree: a uniform grid that hands out the draw orders the
//! R-tree in `crate::bounds_tree` does, and replays last frame's orderings
//! while a frame inserts the bounds the last one did.
//!
//! Adopted from gpui-fast's `crates/gpui/src/fast/bounds_tree.rs` (`b8f0c02`),
//! which `Scene` uses for all three draw-order bands (normal, positioned and
//! top layer). gpux adds the counts fast mode's stats report (`replayed`) and
//! the diagnostics its frame telemetry reads, and keeps upstream's R-tree as
//! the reference the `fast::tests::bounds_grid` group compares with. It adds
//! what `crate::fast::edge_shadows` orders by at a clip's edge: bounds
//! inserted at an ordering worked out elsewhere (`insert_at`), what the tree
//! holds over some bounds (`max_over`), and what it held when so many bounds
//! had been inserted (`max_behind`, [`History`]).

use crate::{Bounds, Half};
use std::{
    fmt::Debug,
    ops::{Add, Sub},
};

/// How many bounds a replay may compare against, counted over every search
/// it makes — for bounds that changed, and for bounds that might meet them —
/// before building the grid costs less than going on without it.
const REPLAY_SEARCH_BUDGET: usize = 1 << 15;

/// The side of a grid cell, in the units of the bounds. A frame's bounds are
/// mostly a few dozen scaled pixels across, so each takes a cell or a few.
const CELL_SIZE: f64 = 64.;

/// The most cells the grid spans along either axis. Bounds reaching past the
/// last cell are kept in it.
const MAX_CELLS_PER_AXIS: usize = 256;

/// No entry: the end of a cell's list.
const NONE: u32 = u32::MAX;

/// Hands out orderings for bounds inserted one after another, each one
/// greater than that of every bounds inserted before it that it intersects,
/// and no greater than it has to be, so that primitives that don't overlap
/// share an ordering and are drawn in one batch.
///
/// The bounds are kept in a uniform grid over the plane. Each cell lists the
/// bounds that reach into it, newest first, except those that cover it
/// whole: every search that reaches into the cell meets those, so the cell
/// keeps only the greatest of their orderings. Upstream kept an R-tree,
/// whose searches and splits cost several times as much for the few
/// thousand bounds of a frame, most of them a cell or a few across.
#[derive(Debug)]
pub(crate) struct BoundsTree<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    grid: Grid<U>,
    /// The bounds with the greatest ordering so far, and that ordering: a
    /// search that meets it has its answer at once.
    max: Option<(Bounds<U>, u32)>,
    /// The bounds inserted since the tree was last cleared, in order, each
    /// with the ordering it was given.
    recorded: Vec<(Bounds<U>, u32)>,
    /// What `recorded` held when the tree was cleared.
    previous: Vec<(Bounds<U>, u32)>,
    /// Which of `recorded` were inserted at an ordering the caller gave
    /// ([`BoundsTree::insert_at`]), by position, ascending.
    given: Vec<u32>,
    /// What `given` held when the tree was cleared.
    previous_given: Vec<u32>,
    /// The first of `recorded`, as far as [`BoundsTree::max_behind`] has
    /// been asked about them, by where they lie.
    history: History<U>,
    /// Whether the tree is still being replayed rather than built: every
    /// ordering since it was cleared has been found without it, and it is
    /// built from `recorded` once that stops paying.
    replaying: bool,
    /// While replaying, every bounds whose entry differs from the one in the
    /// same position in `previous` — in its bounds or in its ordering — in
    /// both its old and its new form. An insert that matches `previous` and
    /// meets none of them has the ordering it had then.
    changed: ChangedBounds<U>,
    /// How many more bounds replaying may compare against, over every search
    /// it makes, before the tree is built instead.
    replay_search_budget: usize,
    /// How many orderings since the tree was cleared were taken from the last
    /// fill rather than searched for, for fast mode's counters.
    #[cfg(any(
        test,
        feature = "test-support",
        feature = "bench-support",
        feature = "diagnostics"
    ))]
    pub(crate) replayed: u32,
    #[cfg(feature = "diagnostics")]
    diagnostics: BoundsTreeDiagnostics,
}

/// What the frame telemetry reports of a bounds tree's work.
#[cfg(feature = "diagnostics")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BoundsTreeDiagnostics {
    /// Bounds inserted.
    pub insertions: u64,
    /// Orderings answered by the greatest bounds so far.
    pub max_leaf_hits: u64,
    /// Orderings searched for in the grid.
    pub slow_searches: u64,
    /// Orderings taken from the last fill.
    pub search_node_visits: u64,
    /// Times the grid was built from what replaying had recorded.
    pub insertion_child_evaluations: u64,
    /// Grid entries held.
    pub retained_nodes: u64,
}

/// The grid a [`BoundsTree`] keeps its bounds in. Cell `(column, row)`
/// spans `column * CELL_SIZE` to `(column + 1) * CELL_SIZE` across, and
/// likewise down, except that the first and last column and row reach on
/// without end, so every point of the plane is in exactly one cell.
#[derive(Debug)]
struct Grid<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    columns: usize,
    rows: usize,
    /// Row by row.
    cells: Vec<Cell>,
    /// Every cell's list, linked through `next`.
    entries: Vec<Entry<U>>,
    /// Bounds without a positive width and height. Such bounds may still
    /// meet others, `intersects` being what it is, but have no cells.
    degenerate: Vec<(Bounds<U>, u32)>,
}

#[derive(Clone, Copy, Debug)]
struct Cell {
    /// The newest entry of the cell's list.
    head: u32,
    /// The greatest ordering among the bounds that cover the cell whole.
    cover: u32,
    /// The greatest ordering among all the cell's bounds, covering or not.
    max: u32,
}

impl Cell {
    const EMPTY: Cell = Cell {
        head: NONE,
        cover: 0,
        max: 0,
    };
}

#[derive(Clone, Debug)]
struct Entry<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    bounds: Bounds<U>,
    order: u32,
    /// The greatest ordering of this entry and every one after it in its
    /// list, so a search stops as soon as the rest can't raise its result.
    rest_max: u32,
    next: u32,
}

/// A bounds' extent along one axis, as the grid sees it.
#[derive(Clone, Copy)]
struct Span {
    start: f64,
    end: f64,
    first_cell: usize,
    last_cell: usize,
}

impl Span {
    fn new(start: f64, end: f64, cells: usize) -> Self {
        Span {
            start,
            end,
            first_cell: cell_at(start, cells),
            last_cell: cell_at(end, cells),
        }
    }

    /// Whether this span reaches into the interior of `cell`, of `cells`,
    /// rather than stopping at one of its edges.
    fn enters(&self, cell: usize, cells: usize) -> bool {
        self.start < cell_end(cell, cells) && self.end > cell_start(cell)
    }

    /// Whether this span covers all of `cell`, of `cells`.
    fn covers(&self, cell: usize, cells: usize) -> bool {
        self.start <= cell_start(cell) && self.end >= cell_end(cell, cells)
    }
}

fn cell_at(coordinate: f64, cells: usize) -> usize {
    // `as` truncates toward zero, which is flooring for all that isn't
    // clamped to the first cell anyway; it saturates, and turns NaN into 0.
    ((coordinate * (1. / CELL_SIZE)) as isize).clamp(0, cells as isize - 1) as usize
}

fn cell_start(cell: usize) -> f64 {
    if cell == 0 {
        f64::NEG_INFINITY
    } else {
        cell as f64 * CELL_SIZE
    }
}

fn cell_end(cell: usize, cells: usize) -> f64 {
    if cell + 1 == cells {
        f64::INFINITY
    } else {
        (cell + 1) as f64 * CELL_SIZE
    }
}

/// How many columns and rows of cells [`ChangedBounds`] marks, each
/// [`CELL_SIZE`] across; the first and last reach on without end, as the
/// grid's do.
const CHANGED_CELLS: usize = 64;

/// The bounds that changed while a tree is replayed, with the cells of a
/// coarse grid each reaches into marked, so that an insert reaching into no
/// marked cell is known to meet none of them without comparing it with each.
///
/// Two bounds that intersect overlap along each axis, and so do the cells
/// they span, which is what a search checks. Bounds whose far edge is not
/// past their near one, or not a number, span no cells to mark or check;
/// once one has changed, or for such a search, every changed bounds is
/// compared.
#[derive(Debug)]
struct ChangedBounds<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    bounds: Vec<Bounds<U>>,
    /// Row by row, a bit per column.
    rows: [u64; CHANGED_CELLS],
    /// Whether a changed bounds spans no cells, so that none can be ruled
    /// out by them.
    unmarked: bool,
}

impl<U> ChangedBounds<U>
where
    U: Clone
        + Debug
        + PartialEq
        + PartialOrd
        + Add<U, Output = U>
        + Sub<Output = U>
        + Half
        + Default
        + Into<f64>,
{
    fn clear(&mut self) {
        self.bounds.clear();
        self.rows = [0; CHANGED_CELLS];
        self.unmarked = false;
    }

    /// The first and last column, and the first and last row, of the cells
    /// `bounds` spans, if it spans any.
    fn cells(bounds: &Bounds<U>) -> Option<(usize, usize, usize, usize)> {
        let left: f64 = bounds.origin.x.clone().into();
        let top: f64 = bounds.origin.y.clone().into();
        let right: f64 = (bounds.origin.x.clone() + bounds.size.width.clone()).into();
        let bottom: f64 = (bounds.origin.y.clone() + bounds.size.height.clone()).into();
        // False for a NaN at either end.
        (right >= left && bottom >= top).then(|| {
            (
                cell_at(left, CHANGED_CELLS),
                cell_at(right, CHANGED_CELLS),
                cell_at(top, CHANGED_CELLS),
                cell_at(bottom, CHANGED_CELLS),
            )
        })
    }

    /// The bits of the columns `first..=last`.
    fn columns(first: usize, last: usize) -> u64 {
        (u64::MAX >> (CHANGED_CELLS - 1 - last)) & (u64::MAX << first)
    }

    fn push(&mut self, bounds: &Bounds<U>) {
        match Self::cells(bounds) {
            Some((left, right, top, bottom)) => {
                let columns = Self::columns(left, right);
                for row in &mut self.rows[top..=bottom] {
                    *row |= columns;
                }
            }
            None => self.unmarked = true,
        }
        self.bounds.push(bounds.clone());
    }

    /// Whether `bounds` might meet a changed bounds: false only when it
    /// cannot.
    fn might_meet(&self, bounds: &Bounds<U>) -> bool {
        if self.bounds.is_empty() {
            return false;
        }
        if self.unmarked {
            return true;
        }
        match Self::cells(bounds) {
            Some((left, right, top, bottom)) => {
                let columns = Self::columns(left, right);
                self.rows[top..=bottom].iter().any(|row| row & columns != 0)
            }
            None => true,
        }
    }
}

impl<U> Grid<U>
where
    U: Clone
        + Debug
        + PartialEq
        + PartialOrd
        + Add<U, Output = U>
        + Sub<Output = U>
        + Half
        + Default
        + Into<f64>,
{
    fn clear(&mut self) {
        self.cells.fill(Cell::EMPTY);
        self.entries.clear();
        self.degenerate.clear();
    }

    /// Whether `bounds` has a positive width and height, and an origin that
    /// is a number, which is what it takes to be kept in cells or searched
    /// for through them.
    #[allow(clippy::eq_op)]
    fn has_area(bounds: &Bounds<U>) -> bool {
        bounds.size.width > U::default()
            && bounds.size.height > U::default()
            && bounds.origin.x == bounds.origin.x
            && bounds.origin.y == bounds.origin.y
    }

    /// Where `bounds` lies along each axis.
    fn spans(&self, bounds: &Bounds<U>) -> (Span, Span) {
        Self::spans_in(self.columns, self.rows, bounds)
    }

    /// Where `bounds` lies along each axis of a grid of `columns` by `rows`.
    fn spans_in(columns: usize, rows: usize, bounds: &Bounds<U>) -> (Span, Span) {
        let right = bounds.origin.x.clone() + bounds.size.width.clone();
        let bottom = bounds.origin.y.clone() + bounds.size.height.clone();
        (
            Span::new(bounds.origin.x.clone().into(), right.into(), columns),
            Span::new(bounds.origin.y.clone().into(), bottom.into(), rows),
        )
    }

    /// How many columns and rows a grid needs to hold `bounds` without
    /// keeping it in its last column or row, if more than it has.
    fn needs(&self, bounds: &Bounds<U>) -> Option<(usize, usize)> {
        Self::needs_in(self.columns, self.rows, bounds)
    }

    /// How many columns and rows a grid of `columns` by `rows` needs to
    /// hold `bounds` without keeping it in its last column or row, if more.
    fn needs_in(columns: usize, rows: usize, bounds: &Bounds<U>) -> Option<(usize, usize)> {
        let right: f64 = (bounds.origin.x.clone() + bounds.size.width.clone()).into();
        let bottom: f64 = (bounds.origin.y.clone() + bounds.size.height.clone()).into();
        if right <= columns as f64 * CELL_SIZE && bottom <= rows as f64 * CELL_SIZE {
            return None;
        }
        let needed = |end: f64| {
            ((end / CELL_SIZE).ceil() as isize).clamp(1, MAX_CELLS_PER_AXIS as isize) as usize
        };
        let (needed_columns, needed_rows) = (needed(right).max(columns), needed(bottom).max(rows));
        (needed_columns > columns || needed_rows > rows).then_some((needed_columns, needed_rows))
    }

    /// Makes the grid `columns` by `rows` and empties it.
    fn resize(&mut self, columns: usize, rows: usize) {
        self.columns = columns;
        self.rows = rows;
        self.cells.clear();
        self.cells.resize(columns * rows, Cell::EMPTY);
        self.entries.clear();
        self.degenerate.clear();
    }

    fn add(&mut self, bounds: &Bounds<U>, order: u32) {
        if !Self::has_area(bounds) {
            self.degenerate.push((bounds.clone(), order));
            return;
        }
        let (x, y) = self.spans(bounds);
        for row in y.first_cell..=y.last_cell {
            let covers_row = y.covers(row, self.rows);
            for column in x.first_cell..=x.last_cell {
                let cell = &mut self.cells[row * self.columns + column];
                cell.max = cell.max.max(order);
                if covers_row && x.covers(column, self.columns) {
                    cell.cover = cell.cover.max(order);
                } else {
                    let rest_max = match self.entries.get(cell.head as usize) {
                        Some(next) => next.rest_max.max(order),
                        None => order,
                    };
                    let entry = self.entries.len() as u32;
                    self.entries.push(Entry {
                        bounds: bounds.clone(),
                        order,
                        rest_max,
                        next: cell.head,
                    });
                    cell.head = entry;
                }
            }
        }
    }

    /// The greatest ordering among the bounds that intersect `query`, which
    /// has a positive width and height, or 0.
    ///
    /// Two such bounds that intersect share a point inside both, and the
    /// cell holding that point lists one of them if it doesn't cover it, and
    /// the query reaches into its interior. A bounds that covers a cell whole
    /// meets every query that reaches into its interior, so the cell's cover
    /// counts there without looking at the bounds.
    fn max_intersecting(&self, query: &Bounds<U>) -> u32 {
        let mut max = self
            .degenerate
            .iter()
            .filter(|(bounds, _)| bounds.intersects(query))
            .map(|(_, order)| *order)
            .max()
            .unwrap_or(0);
        let (x, y) = self.spans(query);
        for row in y.first_cell..=y.last_cell {
            let enters_row = y.enters(row, self.rows);
            for column in x.first_cell..=x.last_cell {
                let cell = &self.cells[row * self.columns + column];
                if cell.max <= max {
                    continue;
                }
                if cell.cover > max && enters_row && x.enters(column, self.columns) {
                    max = cell.cover;
                }
                let mut entry_ix = cell.head;
                while let Some(entry) = self.entries.get(entry_ix as usize) {
                    if entry.rest_max <= max {
                        break;
                    }
                    if entry.order > max && entry.bounds.intersects(query) {
                        max = entry.order;
                    }
                    entry_ix = entry.next;
                }
            }
        }
        max
    }
}

/// The first bounds inserted into a tree, each with its place among them:
/// what the tree held when so many had been inserted, for
/// [`BoundsTree::max_behind`]. A grid as the tree's own is, except that a
/// cell lists the bounds that cover it whole too, those that raised the
/// greatest ordering among them, since which bounds cover a cell depends on
/// how many are looked at; and that it begins where the bounds it holds
/// do, with cells as large as it takes to span them all: bounds past a
/// grid's last cells all lie in them, and content is ordered where it
/// stands, which can be far from the origin.
#[derive(Debug)]
struct History<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    columns: usize,
    rows: usize,
    /// Row by row: the newest entry of each cell's list, and the greatest
    /// ordering among the bounds that cover the cell whole.
    cells: Vec<(u32, u32)>,
    entries: Vec<Past<U>>,
    /// Bounds without a positive width and height, with their places.
    degenerate: Vec<(u32, Bounds<U>, u32)>,
    /// How many of the tree's bounds are held.
    held: usize,
    /// How many times [`CELL_SIZE`] a cell is across: a power of two.
    scale: f64,
    /// Where the grid begins: no further right or down than the bounds
    /// held do. `None` while it holds none.
    near: Option<(f64, f64)>,
    /// The furthest right and bottom edges among the bounds held.
    far: (f64, f64),
}

/// What a [`History`] begins at a multiple of, so that bounds coming a
/// little left of or above those it holds do not move it each time.
const HISTORY_ANCHOR: f64 = 2048.;

/// The most cells a [`History`] has: content is mostly much taller than it
/// is wide, or the other way round, so its grid may span far along one axis
/// where the tree's own, which a window's frame fills, spans as far along
/// both.
const HISTORY_CELLS: usize = 1 << 18;

#[derive(Clone, Debug)]
struct Past<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    bounds: Bounds<U>,
    order: u32,
    /// The greatest ordering of this entry and every one after it in its
    /// list.
    rest_max: u32,
    next: u32,
    /// The bounds' place among those inserted.
    position: u32,
    /// Whether the bounds cover the cell whole.
    covers: bool,
}

impl<U> History<U>
where
    U: Clone
        + Debug
        + PartialEq
        + PartialOrd
        + Add<U, Output = U>
        + Sub<Output = U>
        + Half
        + Default
        + Into<f64>,
{
    fn clear(&mut self) {
        if self.held > 0 {
            self.cells.fill((NONE, 0));
            self.entries.clear();
            self.degenerate.clear();
            self.held = 0;
            self.scale = 1.;
            self.near = None;
            self.far = (0., 0.);
        }
    }

    /// Where `bounds` lies along each axis, in cells.
    fn spans(&self, bounds: &Bounds<U>) -> (Span, Span) {
        let right = bounds.origin.x.clone() + bounds.size.width.clone();
        let bottom = bounds.origin.y.clone() + bounds.size.height.clone();
        let per_cell = 1. / self.scale;
        let near = self.near.unwrap_or_default();
        let along = |start: U, end: U, near: f64, cells: usize| {
            Span::new(
                (Into::<f64>::into(start) - near) * per_cell,
                (Into::<f64>::into(end) - near) * per_cell,
                cells,
            )
        };
        (
            along(bounds.origin.x.clone(), right, near.0, self.columns),
            along(bounds.origin.y.clone(), bottom, near.1, self.rows),
        )
    }

    /// Takes in `recorded`, the first bounds inserted, those of them it
    /// does not hold yet.
    fn hold(&mut self, recorded: &[(Bounds<U>, u32)]) {
        if recorded.len() <= self.held {
            return;
        }
        let mut far = self.far;
        let mut near = self.near;
        for (bounds, _) in &recorded[self.held..] {
            if !Grid::has_area(bounds) {
                continue;
            }
            let (left, top): (f64, f64) = (
                bounds.origin.x.clone().into(),
                bounds.origin.y.clone().into(),
            );
            let right: f64 = (bounds.origin.x.clone() + bounds.size.width.clone()).into();
            let bottom: f64 = (bounds.origin.y.clone() + bounds.size.height.clone()).into();
            if !(left.is_finite() && top.is_finite() && right.is_finite() && bottom.is_finite()) {
                continue;
            }
            far = (far.0.max(right), far.1.max(bottom));
            let anchor = |at: f64| (at / HISTORY_ANCHOR).floor() * HISTORY_ANCHOR;
            near = Some(match near {
                Some(near) if near.0 <= left && near.1 <= top => near,
                Some(near) => (near.0.min(anchor(left)), near.1.min(anchor(top))),
                None => {
                    far = (right, bottom);
                    (anchor(left), anchor(top))
                }
            });
        }
        // Cells large enough for the grid to span them all.
        let begins = near.unwrap_or_default();
        let (across, down) = (far.0 - begins.0, far.1 - begins.1);
        let cells = |extent: f64, scale: f64| ((extent / (CELL_SIZE * scale)).ceil()).max(1.);
        let mut scale = self.scale;
        while cells(across, scale) * cells(down, scale) > HISTORY_CELLS as f64
            && scale < (1u64 << 40) as f64
        {
            scale *= 2.;
        }
        let needed = |extent: f64| (cells(extent, scale) as usize).clamp(1, HISTORY_CELLS);
        let (mut columns, mut rows) = (needed(across), needed(down));
        if scale == self.scale && near == self.near {
            (columns, rows) = (columns.max(self.columns), rows.max(self.rows));
        }
        if (columns, rows, scale, near) != (self.columns, self.rows, self.scale, self.near) {
            self.held = 0;
            self.entries.clear();
            self.degenerate.clear();
            self.cells.clear();
            self.cells.resize(columns * rows, (NONE, 0));
            (self.columns, self.rows, self.scale, self.near) = (columns, rows, scale, near);
        }
        self.far = far;
        for (position, (bounds, order)) in recorded.iter().enumerate().skip(self.held) {
            self.add(bounds, *order, position as u32);
        }
        self.held = recorded.len();
    }

    fn add(&mut self, bounds: &Bounds<U>, order: u32, position: u32) {
        if !Grid::has_area(bounds) {
            self.degenerate.push((position, bounds.clone(), order));
            return;
        }
        let (x, y) = self.spans(bounds);
        for row in y.first_cell..=y.last_cell {
            let covers_row = y.covers(row, self.rows);
            for column in x.first_cell..=x.last_cell {
                let (head, cover) = &mut self.cells[row * self.columns + column];
                let covers = covers_row && x.covers(column, self.columns);
                if covers {
                    // Under bounds that cover the cell too, and came first.
                    if order <= *cover {
                        continue;
                    }
                    *cover = order;
                }
                let rest_max = match self.entries.get(*head as usize) {
                    Some(next) => next.rest_max.max(order),
                    None => order,
                };
                let entry = self.entries.len() as u32;
                self.entries.push(Past {
                    bounds: bounds.clone(),
                    order,
                    rest_max,
                    next: *head,
                    position,
                    covers,
                });
                *head = entry;
            }
        }
    }

    /// The greatest ordering among the first `count` bounds that intersect
    /// `query`, which has a positive width and height, or 0; and how many
    /// entries were looked at. See [`Grid::max_intersecting`].
    fn max_among_first(&self, count: usize, query: &Bounds<U>) -> (u32, usize) {
        let count = count as u32;
        let mut max = self
            .degenerate
            .iter()
            .filter(|(position, bounds, _)| *position < count && bounds.intersects(query))
            .map(|(_, _, order)| *order)
            .max()
            .unwrap_or(0);
        let mut looked = self.degenerate.len();
        let (x, y) = self.spans(query);
        for row in y.first_cell..=y.last_cell {
            let enters_row = y.enters(row, self.rows);
            for column in x.first_cell..=x.last_cell {
                let enters = enters_row && x.enters(column, self.columns);
                let mut entry_ix = self.cells[row * self.columns + column].0;
                while let Some(entry) = self.entries.get(entry_ix as usize) {
                    looked += 1;
                    entry_ix = entry.next;
                    // Inserted after those asked about.
                    if entry.position >= count {
                        continue;
                    }
                    if entry.rest_max <= max {
                        break;
                    }
                    let meets = match entry.covers {
                        true => enters,
                        false => entry.bounds.intersects(query),
                    };
                    if entry.order > max && meets {
                        max = entry.order;
                    }
                }
            }
        }
        (max, looked)
    }
}

impl<U> BoundsTree<U>
where
    U: Clone
        + Debug
        + PartialEq
        + PartialOrd
        + Add<U, Output = U>
        + Sub<Output = U>
        + Half
        + Default
        + Into<f64>,
{
    /// Clears all bounds from the tree.
    ///
    /// What was inserted since the last clear is kept aside: a frame usually
    /// inserts the same bounds in the same order as the one before it, and an
    /// ordering depends on nothing but the bounds inserted before it, so as
    /// long as that holds the orderings can be handed out again as they were.
    pub fn clear(&mut self) {
        self.grid.clear();
        self.max = None;
        std::mem::swap(&mut self.previous, &mut self.recorded);
        self.recorded.clear();
        std::mem::swap(&mut self.previous_given, &mut self.given);
        self.given.clear();
        self.history.clear();
        self.replaying = true;
        self.changed.clear();
        self.replay_search_budget = REPLAY_SEARCH_BUDGET;
        #[cfg(any(
            test,
            feature = "test-support",
            feature = "bench-support",
            feature = "diagnostics"
        ))]
        {
            self.replayed = 0;
        }
        #[cfg(feature = "diagnostics")]
        {
            self.diagnostics = BoundsTreeDiagnostics::default();
        }
    }

    /// Clears the tree and forgets what was inserted before, so the next fill
    /// is ordered from scratch rather than replayed.
    pub fn forget(&mut self) {
        self.clear();
        self.previous.clear();
        self.previous_given.clear();
        self.replaying = false;
    }

    /// The greatest ordering among the bounds in the tree that `query`
    /// meets, or 0 when it meets none. For a tree filled through
    /// [`BoundsTree::insert_at`], which is never replayed.
    pub(crate) fn max_over(&mut self, query: &Bounds<U>) -> u32 {
        debug_assert!(!self.replaying);
        self.find_max_ordering(query)
    }

    /// Whether the bounds at `position` of the last fill were inserted at an
    /// ordering the caller gave.
    fn was_given(&self, position: usize) -> bool {
        self.previous_given
            .binary_search(&(position as u32))
            .is_ok()
    }

    /// The greatest ordering among the first `count` bounds inserted that
    /// `query` meets, or 0 when it meets none: what the tree would have
    /// answered before the bounds after them were inserted. With how many
    /// bounds were looked at.
    ///
    /// The first `count` bounds are put in a grid of their own the first
    /// time they are asked about, each with its place among them, so that
    /// the answer costs what lies near `query` and not what was inserted.
    pub(crate) fn max_behind(&mut self, count: usize, query: &Bounds<U>) -> (u32, usize) {
        let count = count.min(self.recorded.len());
        if !Grid::has_area(query) {
            let max = self.recorded[..count]
                .iter()
                .filter(|(bounds, _)| bounds.intersects(query))
                .map(|(_, ordering)| *ordering)
                .max()
                .unwrap_or(0);
            return (max, count);
        }
        self.history.hold(&self.recorded[..count]);
        self.history.max_among_first(count, query)
    }

    /// How many bounds have been inserted since the tree was cleared.
    pub(crate) fn inserted(&self) -> usize {
        self.recorded.len()
    }

    /// Sizes the grid to hold `bounds`, so that filling it with bounds
    /// spread over them does not grow it step by step.
    pub(crate) fn cover(&mut self, bounds: &Bounds<U>) {
        debug_assert!(!self.replaying);
        if let Some((columns, rows)) = self.grid.needs(bounds) {
            self.grid.resize(columns, rows);
            for (recorded, ordering) in &self.recorded {
                self.grid.add(recorded, *ordering);
            }
        }
    }

    /// Adds `bounds` at `ordering`, which the caller worked out: the tree
    /// then answers for it, without having ordered it. See
    /// [`BoundsTree::max_over`].
    pub(crate) fn insert_at(&mut self, bounds: Bounds<U>, ordering: u32) {
        let position = self.recorded.len();
        self.given.push(position as u32);
        if self.replaying {
            // As an insertion replayed: what it met last time and what it
            // meets now are compared by those that come after it. Bounds
            // the tree ordered itself last time are not the same insertion.
            let given_before = self.was_given(position);
            match self.previous.get(position) {
                Some((previous, previous_ordering))
                    if given_before && *previous == bounds && *previous_ordering == ordering => {}
                Some((previous, _)) => {
                    let previous = previous.clone();
                    self.changed.push(&previous);
                    self.changed.push(&bounds);
                }
                None => self.changed.push(&bounds),
            }
            self.recorded.push((bounds, ordering));
            return;
        }
        self.add(&bounds, ordering);
        self.recorded.push((bounds, ordering));
    }

    /// Inserts bounds into the tree and returns its assigned ordering.
    ///
    /// The ordering is one greater than the maximum ordering of any
    /// existing bounds that intersect with the new bounds.
    pub fn insert(&mut self, new_bounds: Bounds<U>) -> u32 {
        #[cfg(feature = "diagnostics")]
        {
            self.diagnostics.insertions += 1;
        }
        if self.replaying {
            if let Some(ordering) = self.replay(&new_bounds) {
                self.recorded.push((new_bounds, ordering));
                return ordering;
            }
            self.replaying = false;
            #[cfg(feature = "diagnostics")]
            {
                self.diagnostics.insertion_child_evaluations += 1;
            }
            self.build_from_recorded();
        }

        let ordering = self.find_max_ordering(&new_bounds) + 1;
        self.add(&new_bounds, ordering);
        self.recorded.push((new_bounds, ordering));
        ordering
    }

    /// The ordering `bounds` is given while the tree is being replayed, or
    /// `None` once finding it without the tree would cost more than building
    /// the tree.
    ///
    /// An ordering is one more than the greatest among the bounds inserted
    /// before it that it meets. If `bounds` is what was inserted at this
    /// position last time, and meets nothing that was, or is now, different
    /// from last time before it, then everything it meets and every ordering
    /// among them is as it was, and so is its own. Otherwise its ordering is
    /// worked out from what has been inserted so far, and if that differs
    /// from last time, the bounds join the ones that changed.
    fn replay(&mut self, bounds: &Bounds<U>) -> Option<u32> {
        let previous = self.previous.get(self.recorded.len());
        // An ordering the caller gave last time is not one this insertion
        // would be given.
        if let Some((previous_bounds, ordering)) = previous
            && previous_bounds == bounds
            && !self.was_given(self.recorded.len())
        {
            let meets_changed = self.changed.might_meet(bounds) && {
                self.replay_search_budget = self
                    .replay_search_budget
                    .checked_sub(self.changed.bounds.len())?;
                self.changed
                    .bounds
                    .iter()
                    .any(|changed| changed.intersects(bounds))
            };
            if !meets_changed {
                #[cfg(any(
                    test,
                    feature = "test-support",
                    feature = "bench-support",
                    feature = "diagnostics"
                ))]
                {
                    self.replayed += 1;
                }
                #[cfg(feature = "diagnostics")]
                {
                    self.diagnostics.search_node_visits += 1;
                }
                return Some(*ordering);
            }
        }

        self.replay_search_budget = self.replay_search_budget.checked_sub(self.recorded.len())?;
        let ordering = self
            .recorded
            .iter()
            .filter(|(other, _)| other.intersects(bounds))
            .map(|(_, ordering)| *ordering)
            .max()
            .unwrap_or(0)
            + 1;
        match previous {
            Some((previous_bounds, previous_ordering)) => {
                if previous_bounds != bounds {
                    self.changed.push(previous_bounds);
                    self.changed.push(bounds);
                } else if *previous_ordering != ordering {
                    self.changed.push(bounds);
                }
            }
            None => self.changed.push(bounds),
        }
        Some(ordering)
    }

    /// Adds `bounds` with `ordering` to the grid, growing it first if the
    /// bounds reach past it.
    fn add(&mut self, bounds: &Bounds<U>, ordering: u32) {
        if Grid::has_area(bounds)
            && let Some((columns, rows)) = self.grid.needs(bounds)
        {
            self.grid.resize(columns, rows);
            for (recorded, recorded_ordering) in &self.recorded {
                self.grid.add(recorded, *recorded_ordering);
            }
        }
        self.grid.add(bounds, ordering);
        if self.max.as_ref().is_none_or(|(_, max)| *max < ordering) {
            self.max = Some((bounds.clone(), ordering));
        }
    }

    /// Builds the grid from what has been inserted since it was cleared,
    /// whose orderings were handed out without it. Their orderings are
    /// known, so there is nothing to search for, only bounds to place.
    fn build_from_recorded(&mut self) {
        let mut size = (self.grid.columns, self.grid.rows);
        for (bounds, _) in &self.recorded {
            if Grid::has_area(bounds)
                && let Some((columns, rows)) = self.grid.needs(bounds)
            {
                size = (size.0.max(columns), size.1.max(rows));
            }
        }
        if size != (self.grid.columns, self.grid.rows) {
            self.grid.resize(size.0, size.1);
        }
        for (bounds, ordering) in &self.recorded {
            self.grid.add(bounds, *ordering);
            if self.max.as_ref().is_none_or(|(_, max)| max < ordering) {
                self.max = Some((bounds.clone(), *ordering));
            }
        }
    }

    /// Finds the maximum ordering among all bounds that intersect with the query.
    fn find_max_ordering(&mut self, query: &Bounds<U>) -> u32 {
        if let Some((max_bounds, max)) = &self.max
            && query.intersects(max_bounds)
        {
            #[cfg(feature = "diagnostics")]
            {
                self.diagnostics.max_leaf_hits += 1;
            }
            return *max;
        }
        #[cfg(feature = "diagnostics")]
        {
            self.diagnostics.slow_searches += 1;
        }
        if Grid::has_area(query) {
            self.grid.max_intersecting(query)
        } else {
            // Without an area of its own the query may miss the interior of
            // every cell it touches, so the covers can't answer for it.
            self.recorded
                .iter()
                .filter(|(bounds, _)| bounds.intersects(query))
                .map(|(_, ordering)| *ordering)
                .max()
                .unwrap_or(0)
        }
    }
}

impl<U> Default for BoundsTree<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    fn default() -> Self {
        BoundsTree {
            grid: Grid {
                columns: 1,
                rows: 1,
                cells: vec![Cell::EMPTY],
                entries: Vec::new(),
                degenerate: Vec::new(),
            },
            max: None,
            recorded: Vec::new(),
            previous: Vec::new(),
            given: Vec::new(),
            previous_given: Vec::new(),
            history: History {
                columns: 1,
                rows: 1,
                cells: vec![(NONE, 0)],
                entries: Vec::new(),
                degenerate: Vec::new(),
                held: 0,
                scale: 1.,
                near: None,
                far: (0., 0.),
            },
            replaying: false,
            changed: ChangedBounds {
                bounds: Vec::new(),
                rows: [0; CHANGED_CELLS],
                unmarked: false,
            },
            replay_search_budget: REPLAY_SEARCH_BUDGET,
            #[cfg(any(
                test,
                feature = "test-support",
                feature = "bench-support",
                feature = "diagnostics"
            ))]
            replayed: 0,
            #[cfg(feature = "diagnostics")]
            diagnostics: BoundsTreeDiagnostics::default(),
        }
    }
}

impl<U> BoundsTree<U>
where
    U: Clone + Debug + Default + PartialEq,
{
    /// The frame telemetry's view of the tree's work since it was cleared.
    #[cfg(feature = "diagnostics")]
    pub(crate) fn diagnostics(&self) -> BoundsTreeDiagnostics {
        BoundsTreeDiagnostics {
            retained_nodes: self.grid.entries.len() as u64,
            ..self.diagnostics
        }
    }
}

impl crate::Scene {
    /// Forgets the orderings the scene's bounds trees would replay, so the
    /// next frame orders every primitive by searching. The oracle's reference
    /// window draws every frame this way.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn forget_orderings(&mut self) {
        self.primitive_bounds.forget();
        self.positioned_primitive_bounds.forget();
        self.top_layer_primitive_bounds.forget();
    }
}
