# Expected outcome

The TextArea remains visually and interactively unchanged. Each source logical line is sliced by
its validated Rope range, and the only line-wide allocation is the text retained by its layout row.
