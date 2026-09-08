# Repository instructions

## Semantic authority

- Keep CRDT operation semantics, merge behavior, invariant checks, deterministic replay, serialization, and measurements authoritative in Rust.
- Browser/WASM code may visualize and drive the Rust implementation; do not create a second JavaScript implementation of CRDT truth.
- Preserve stable replica, item/node, and operation identities. Do not model first-class moves as delete-plus-insert when identity is part of the experiment.

## Evaluation model

Keep these results separate in code, tests, documentation, and UI:

1. convergence after replicas observe the same operations;
2. structural validity and invariant preservation;
3. intent quality for concurrent user actions.

Convergence alone is not a success criterion.

## Determinism

- Make partitions, delayed/reordered/duplicate delivery, and reconnection reproducible.
- Preserve idempotent duplicate handling and deterministic replay from serialized scenarios or seeds.
- Prefer small adversarial scenarios that explain a semantic failure over opaque aggregate scores.

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
