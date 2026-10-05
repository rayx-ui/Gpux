# Expected outcome

The focused TextArea wheel action stays visible and stable. Changed layouts access prior rows by
borrow and only clone each retained `Arc` for the next cache, eliminating the full map clone.
