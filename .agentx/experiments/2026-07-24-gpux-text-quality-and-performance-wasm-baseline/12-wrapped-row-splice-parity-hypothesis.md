# Hypothesis

For a same-length, single-line edit that changes the wrapped visual-row count, the retained layout map updates only the edited row and shifts the following visual-row starts without rebuilding unaffected rows. Full layout remains the oracle for every buffer/display coordinate.
