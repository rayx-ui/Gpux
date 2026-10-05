# Hypothesis

Borrowing the installed row-layout map during changed-layout preparation removes its full
`HashMap` clone while preserving the existing atomic install and error behavior. Individual
`Arc<WrappedLineLayout>` values remain cloned only when retained by the next installed map.

## Scope lock

This experiment changes only `TextLayoutCache::reusable_rows` and its call site. It does not alter
dirty-row detection, text materialization, row metadata, or prepaint.
