# CRDT Lab

CRDT Lab is an experimental laboratory for conflict-free replicated data types, with an emphasis on the parts that become difficult once the replicated value has structure: ordered sequences, first-class moves, trees, DAGs, and general graphs.

The project deliberately separates three questions that are often conflated:

1. **Convergence** — do replicas that have observed the same operations reach the same state?
2. **Structural validity** — does the converged value still satisfy its invariants, such as acyclicity or single-parent ownership?
3. **Intent quality** — is the deterministic result a reasonable interpretation of what concurrent users tried to do?

An algorithm is not considered successful merely because it converges.

## Goals

- Compare CRDT algorithms under exactly the same deterministic histories.
- Make network partitions, delayed delivery, duplicated delivery, reordered delivery, and reconnection explicit parts of experiments.
- Preserve stable object identity so that moves can be studied independently from delete-and-reinsert semantics.
- Test invariants and user-facing semantics separately from convergence.
- Keep algorithmic truth in Rust and expose the same implementation to the browser through WASM.
- Provide a GitHub Pages lab where scenarios can be created, replayed, inspected, exported, and compared side by side.
- Make every interesting failure reproducible from a small serialized scenario and deterministic seed.

## Core experiment model

Every experiment consists of named replicas, operations created at those replicas, and an explicit delivery schedule. A run records local state after each operation and delivery step.

The harness must eventually support:

- offline edits and arbitrary partitions;
- delayed, reordered, and duplicated messages;
- exact replay from a seed or serialized event log;
- two, three, or more replicas;
- side-by-side execution of multiple algorithms over an equivalent semantic history;
- convergence checks after all operations have been delivered;
- structural invariant checks after every local and merged transition;
- semantic assertions for curated adversarial scenarios;
- performance, metadata-growth, tombstone, and synchronization-cost measurements.

## Research tracks

### 1. Sequence CRDTs

Compare representative families rather than treating text/list CRDTs as one design:

- RGA-style linked/tombstoned sequences;
- Logoot and LSEQ-style position identifiers;
- YATA-style ordering;
- Fugue and FugueMax-style sequence semantics;
- newer dense-identifier approaches such as ESBT.

Questions to measure include concurrent insertion interleaving, position-identifier growth, tombstone growth, metadata overhead, insertion/move cost, and whether adjacent edits remain perceptually adjacent after a merge.

### 2. First-class movable ordered lists

A move is not assumed to be equivalent to delete plus insert. Items retain permanent identity while their position changes.

Important conflicts include:

- concurrent moves of the same item;
- move versus delete;
- move versus concurrent insertion beside the moved item;
- repeated moves during partitions;
- range moves;
- whether an edit attached to an item follows that item when it moves.

The lab should compare simple last-writer baselines against algorithms whose semantics explicitly preserve more ordering intent.

### 3. Movable trees

Nodes retain identity and may be created, deleted, reparented, and eventually reordered among siblings.

The canonical adversarial case is a pair of individually valid concurrent moves that form a cycle when combined, such as moving A below B on one replica while moving B below A on another.

The lab must distinguish deterministic conflict resolution from invariant-preserving conflict resolution.

### 4. Ordered movable trees

Study parent choice and sibling ordering both independently and as one operation. Compare compositions of a parent/move CRDT plus an ordering CRDT against unified algorithms.

Important questions:

- Can a subtree move retain stable identity and all concurrent edits inside the subtree?
- What happens when the same node is concurrently moved to different parents?
- What happens when parent choice and sibling position are concurrently changed?
- Can a deterministic cycle-breaking policy avoid surprising relocation?
- Does fractional/dense indexing remain practical under long move histories?

### 5. Graph and DAG CRDTs

Start with vertices and edges built from replicated sets, then impose stronger invariants:

- no dangling edges;
- uniqueness constraints;
- DAG-only structure;
- reachability requirements;
- optional single-parent constraints;
- typed or property-graph constraints.

Pay particular attention to conflicts such as concurrent vertex deletion and edge creation. Add-wins and remove-wins are useful baselines, not universal answers.

### 6. Semantic torture tests

Property-based and generated histories should run at high volume, but the project also needs a curated corpus of small scenarios with explicit expected semantics.

Examples:

- Alice moves C after F while Bob moves C before A.
- Carla inserts X beside C while C is concurrently moved.
- Alice reparents A below B while Bob reparents B below A.
- One replica deletes a graph vertex while another creates an edge to it.
- Multiple replicas repeatedly reorder the same sibling set while disconnected.

Every algorithm reports convergence, invariants, and semantic expectations separately.

## Recent research directions to track

The lab should continuously absorb useful ideas from current CRDT research. In particular:

- **Sequence identifier growth:** recent dense-identifier work such as ESBT targets the long-history metadata-growth problem seen in Logoot/LSEQ families.
- **Declarative CRDT semantics:** recent Datalog-oriented work suggests specifying concurrency semantics independently of implementation and then checking implementations against that specification.
- **Efficient state synchronization:** work such as ConflictSync focuses on reconciling the irredundant pieces of state-based CRDTs rather than repeatedly transferring entire states.
- **Invariant preservation without general coordination:** deterministic no-op/conflict policies are interesting for operations that cannot coexist while preserving an invariant.
- **Replicated DAGs and property graphs:** graph replication increasingly focuses on the interaction between availability and structural/domain constraints, not just replicated vertex and edge sets.
- **Security and Byzantine replicas:** explore proof-carrying updates, bounded damage from malicious replicas, and authority changes whose concurrent interpretation has security consequences.
- **Automated CRDT engineering:** track work on deriving compaction/redundancy rules, executable specifications, contracts, and formal/property verification for custom CRDTs.

## Architecture direction

Rust owns operation semantics, merge behavior, invariant checks, deterministic replay, serialization, and measurements.

The browser is a visualization and interaction surface over the same Rust implementation compiled to WASM. It must not contain a second JavaScript implementation of CRDT truth.

The intended browser lab contains:

- one pane per replica;
- editable partition/delivery controls;
- a timeline of local operations and message deliveries;
- a visual sequence/tree/graph representation;
- an explanation of why an algorithm chose a particular result;
- side-by-side algorithm comparison;
- scenario import/export;
- reproducible seeds for generated histories;
- concise convergence, invariant, and semantic-result reporting rather than decorative counters.

## Roadmap

### Slice 1 — Deterministic replica harness + movable-list baseline

Build the smallest complete laboratory loop:

- stable replica, item, and operation identities;
- deterministic Lamport ordering with replica-id tie breaking;
- idempotent operation application;
- explicit out-of-order and duplicate delivery;
- a small ordered-list baseline with first-class move operations;
- convergence tests over multiple delivery orders;
- move-vs-move, move-vs-delete, and duplicate-delivery tests;
- deterministic snapshot serialization.

This is deliberately a baseline, not a claim that last-writer move semantics are ideal. Later sequence algorithms must be measurable against it.

### Slice 2 — Scenario runner and adversarial corpus

Introduce a serializable experiment format, named replicas, partitions, delivery steps, semantic assertions, seeded generated histories, and replay diagnostics.

### Slice 3 — Sequence algorithm comparison

Add RGA, LSEQ/Logoot, YATA, Fugue/FugueMax, and an ESBT experiment. Normalize them behind a common experiment interface without erasing algorithm-specific metadata.

### Slice 4 — Move semantics laboratory

Compare delete-plus-insert, last-writer move, and dedicated move-aware sequence semantics. Add concurrent neighbor edits and range moves.

### Slice 5 — Movable tree

Add reparenting with permanent node identity, concurrent-move handling, deterministic cycle prevention, subtree preservation, and invariant tests.

### Slice 6 — Ordered movable tree

Add sibling ordering and compare composed parent-plus-ordering schemes against unified move/reorder semantics.

### Slice 7 — Graph and DAG laboratory

Add observed-remove graph baselines followed by dangling-edge, DAG, reachability, and property constraints with explicit conflict policies.

### Slice 8 — Semantic oracle and explanation

Give curated scenarios human-intent expectations and require each algorithm to explain the causal/conflict decisions that produced its result.

### Slice 9 — Synchronization and storage costs

Measure metadata growth, tombstones, state size, delta/op sizes, compaction opportunities, and full-state versus decomposed/delta synchronization.

### Slice 10 — Verification and hostile replicas

Add declarative/model-based semantics, property testing, deterministic fuzzing, malformed or malicious update handling, and security-sensitive authority experiments.

### Slice 11 — WASM and GitHub Pages laboratory

Compile the Rust core to WASM and expose the experiment runner as a static GitHub Pages application with replica panes, network controls, timelines, visual structures, comparisons, and scenario import/export.

## Non-goals

- Hiding semantic tradeoffs behind a single generic `CRDT` abstraction.
- Claiming that convergence alone makes an algorithm suitable for collaboration.
- Reimplementing algorithm truth in the web layer.
- Treating move as delete plus insert when identity matters.
- Introducing a server requirement for experiments that can be reproduced locally or in a static Pages deployment.

## Definition of done for an algorithm

A CRDT implementation added to the lab should have:

- a stated operation and conflict model;
- deterministic replay;
- duplicate-delivery/idempotence coverage;
- convergence tests across delivery permutations;
- explicit invariant checks;
- adversarial semantic scenarios;
- deterministic serialization;
- measurements appropriate to its design;
- documentation of known semantic compromises.
