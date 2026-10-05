# Decision

Accept the borrowed reusable-row map. It removes a full allocation and `Arc` cloning pass from
changed-layout preparation without adding state, invalidation rules, or failure paths.
