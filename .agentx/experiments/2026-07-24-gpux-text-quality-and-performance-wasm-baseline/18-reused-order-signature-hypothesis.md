## Reused SelectionArea geometry signature buffer

Retain an area-owned scratch buffer for the next geometry signature so unchanged frames compare revisions without allocating a new participant-sized signature vector. Keep the existing bounds index and geometry semantics unchanged.

