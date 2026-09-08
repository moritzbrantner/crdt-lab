# Movable-list baseline semantics

Slice 1 intentionally starts with a simple model whose compromises are easy to observe.

## State

Each item has permanent identity plus three independently versioned pieces of replicated state:

- its latest creation/value;
- its latest placement;
- its latest deletion timestamp.

Operations are identified and totally ordered by a Lamport counter followed by replica id. Position values are dense lexicographic digit sequences allocated between the item's visible neighbors.

## Conflict policy

- A newer insert for the same identity wins the value register.
- A newer insert or move wins the placement register.
- The newest delete timestamp is retained.
- An item is visible when its winning placement is newer than its winning delete.
- Equal position values are rendered by placement timestamp and then item id.
- Duplicate operation ids are ignored.

This means a concurrent move can beat a delete and make an item visible, or a concurrent delete can beat a move and hide it. The replica-id tie breaker is intentionally arbitrary with respect to human intent. Both behaviors are part of the baseline, not desired end-state semantics.

## Why keep this baseline

The model gives CRDT Lab a small convergent reference with:

- stable item identity;
- first-class moves;
- out-of-order delivery safety, including moves received before inserts;
- explicit duplicate-delivery idempotence;
- deterministic serialization;
- deterministic concurrent conflict resolution.

Later algorithms should be compared against this model rather than replacing it. A more sophisticated algorithm earns its complexity only when it improves measurable behavior such as insertion adjacency, move intent, metadata growth, structural invariants, or synchronization cost.

## Known limitations

- Lamport/replica last-writer ordering does not understand user intent.
- A move may resurrect an item after a conflicting delete.
- Position allocation is a deliberately simple dense-index scheme, not Logoot, LSEQ, ESBT, or another researched sequence identifier design.
- Position identifiers can grow with pathological repeated insertion into the same gap.
- There is no range move yet.
- There is no causal-context-aware delete policy yet.
- There is no scenario file format or generated-history runner yet.
- The model handles an ordered list only; tree and graph invariants arrive in later slices.

These limitations are experiment targets rather than hidden implementation details.
