# Expected outcome

Scrolling the large populated TextArea remains visibly unchanged, including selection and the
scrolled viewport. The placeholder remains present only for empty content, while prepaint no
longer allocates a whole text string merely to detect that condition.
