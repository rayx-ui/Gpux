## Registrar-gated DataTable structured selection frame

Start `TableSelectionModel` frame collection only when the current traversal exposes a SelectionArea registrar. Outside an area, clear stale structured selection and preserve the table child unchanged.

