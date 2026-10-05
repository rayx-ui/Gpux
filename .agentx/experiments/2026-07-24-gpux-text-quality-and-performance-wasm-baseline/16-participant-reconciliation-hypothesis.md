## Revisioned SelectionArea participant reconciliation

Replace per-frame participant ID vectors and temporary `HashSet`s with an area-owned revisioned position map. Reconcile registration identity and order from the next traversal in one pass, retaining only active participants without participant-sized temporary sets.

