# Decision

Accept direct row lookup. It removes repeated linear searches without changing row ownership or
adding index state that must be invalidated after edits.
