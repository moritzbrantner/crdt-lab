use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{ApplyOutcome, ItemId, ReplicaId, Timestamp};

/// Stable RGA node identity.
///
/// Inserts use their Lamport operation timestamp as the permanent node identity.
/// The wrapper makes the RGA-specific role explicit without inventing a second
/// clock domain.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RgaId(Timestamp);

impl RgaId {
    /// Returns the timestamp that identifies this RGA node.
    #[must_use]
    pub const fn timestamp(&self) -> &Timestamp {
        &self.0
    }
}

/// Operation understood by the RGA sequence implementation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaOperation {
    timestamp: Timestamp,
    kind: RgaOperationKind,
}

impl RgaOperation {
    /// Returns the stable operation timestamp.
    #[must_use]
    pub const fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    /// Returns the operation payload.
    #[must_use]
    pub const fn kind(&self) -> &RgaOperationKind {
        &self.kind
    }
}

/// RGA operation payload.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RgaOperationKind {
    /// Inserts a new permanent node after another node, or at the root.
    Insert {
        /// Permanent node identity.
        node: RgaId,
        /// Predecessor selected from the local visible sequence.
        after: Option<RgaId>,
        /// Stable semantic item identity used by the experiment harness.
        item: ItemId,
        /// Opaque sequence value.
        value: String,
    },
    /// Tombstones an existing node.
    Delete {
        /// Permanent node identity to tombstone.
        target: RgaId,
    },
}

/// Error returned while creating a local RGA operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RgaError {
    /// The requested insertion index is outside the visible sequence.
    IndexOutOfBounds {
        /// Requested insertion index.
        index: usize,
        /// Maximum accepted insertion index.
        len: usize,
    },
    /// Stable semantic item identity was already used on this replica.
    ItemAlreadyExists(ItemId),
    /// A delete targeted an item that is not currently visible.
    ItemNotVisible(ItemId),
    /// The local Lamport clock cannot allocate another timestamp.
    ClockExhausted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RgaNode {
    after: Option<RgaId>,
    item: ItemId,
    value: String,
}

/// One visible item in rendered RGA order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaVisibleItem {
    /// Stable semantic item identity.
    pub id: ItemId,
    /// Opaque sequence value.
    pub value: String,
    /// Permanent algorithm-specific node identity.
    pub node: RgaId,
}

/// Algorithm-specific metadata for one RGA node.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaNodeMetadata {
    /// Permanent node identity.
    pub node: RgaId,
    /// Predecessor relation used by RGA.
    pub after: Option<RgaId>,
    /// Stable semantic item identity.
    pub item: ItemId,
    /// Opaque sequence value.
    pub value: String,
    /// Whether the node is currently tombstoned.
    pub tombstone: bool,
}

/// Deterministic RGA snapshot.
///
/// The visible result and the full algorithm-specific metadata are deliberately
/// separate so comparisons do not erase predecessor or tombstone information.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaSnapshot {
    /// Visible items in rendered sequence order.
    pub items: Vec<RgaVisibleItem>,
    /// Every known node in stable node-id order, including tombstones.
    pub nodes: Vec<RgaNodeMetadata>,
}

/// Structural invariant violation detected in an RGA replica.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RgaInvariantViolation {
    /// A node refers to a predecessor that has not been observed.
    MissingPredecessor {
        /// Node with the dangling predecessor.
        node: RgaId,
        /// Missing predecessor identity.
        predecessor: RgaId,
    },
    /// Following predecessor links reaches the same node twice.
    Cycle {
        /// Node from which the cycle was detected.
        node: RgaId,
    },
    /// Multiple RGA nodes claim the same experiment-level stable item identity.
    DuplicateItemIdentity {
        /// Duplicate semantic identity.
        item: ItemId,
        /// Nodes carrying the duplicate identity.
        nodes: Vec<RgaId>,
    },
}

/// Replicated Growable Array sequence.
///
/// Nodes are never physically removed. Inserts identify their predecessor and
/// concurrent siblings are ordered by descending node id, which makes a later
/// local insertion immediately follow the chosen predecessor while remaining
/// deterministic after merge.
#[derive(Clone, Debug)]
pub struct RgaReplica {
    replica: ReplicaId,
    clock: u64,
    seen: BTreeSet<Timestamp>,
    nodes: BTreeMap<RgaId, RgaNode>,
    tombstones: BTreeSet<RgaId>,
}

impl RgaReplica {
    /// Creates an empty RGA replica.
    #[must_use]
    pub fn new(replica: ReplicaId) -> Self {
        Self {
            replica,
            clock: 0,
            seen: BTreeSet::new(),
            nodes: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }

    /// Creates and locally applies an insert at a visible sequence index.
    ///
    /// # Errors
    ///
    /// Returns RgaError::IndexOutOfBounds for an invalid index,
    /// RgaError::ItemAlreadyExists when the stable semantic identity is already
    /// known, or RgaError::ClockExhausted when the Lamport clock is exhausted.
    pub fn insert_at(
        &mut self,
        index: usize,
        item: ItemId,
        value: impl Into<String>,
    ) -> Result<RgaOperation, RgaError> {
        if self.nodes.values().any(|node| node.item == item) {
            return Err(RgaError::ItemAlreadyExists(item));
        }

        let visible = self.visible_items();
        if index > visible.len() {
            return Err(RgaError::IndexOutOfBounds {
                index,
                len: visible.len(),
            });
        }

        let after = index
            .checked_sub(1)
            .and_then(|previous| visible.get(previous))
            .map(|item| item.node.clone());

        let timestamp = self.next_timestamp()?;
        let operation = RgaOperation {
            timestamp: timestamp.clone(),
            kind: RgaOperationKind::Insert {
                node: RgaId(timestamp),
                after,
                item,
                value: value.into(),
            },
        };

        let outcome = self.apply(&operation);
        debug_assert_eq!(outcome, ApplyOutcome::Applied);
        Ok(operation)
    }

    /// Creates and locally applies a tombstone operation for a visible item.
    ///
    /// # Errors
    ///
    /// Returns RgaError::ItemNotVisible if the stable item identity is not
    /// visible or RgaError::ClockExhausted if no operation timestamp can be
    /// allocated.
    pub fn delete(&mut self, item: &ItemId) -> Result<RgaOperation, RgaError> {
        let target = self
            .visible_items()
            .into_iter()
            .find(|visible| &visible.id == item)
            .map(|visible| visible.node)
            .ok_or_else(|| RgaError::ItemNotVisible(item.clone()))?;

        let operation = RgaOperation {
            timestamp: self.next_timestamp()?,
            kind: RgaOperationKind::Delete { target },
        };

        let outcome = self.apply(&operation);
        debug_assert_eq!(outcome, ApplyOutcome::Applied);
        Ok(operation)
    }

    /// Applies a local or remote RGA operation.
    ///
    /// Insert operations may arrive before their predecessor. They remain in
    /// replicated state and become renderable when the predecessor arrives.
    /// Delete operations may likewise arrive before the target insert.
    pub fn apply(&mut self, operation: &RgaOperation) -> ApplyOutcome {
        if !self.seen.insert(operation.timestamp.clone()) {
            return ApplyOutcome::Duplicate;
        }

        self.clock = self.clock.max(operation.timestamp.counter());

        match &operation.kind {
            RgaOperationKind::Insert {
                node,
                after,
                item,
                value,
            } => {
                self.nodes.entry(node.clone()).or_insert_with(|| RgaNode {
                    after: after.clone(),
                    item: item.clone(),
                    value: value.clone(),
                });
            }
            RgaOperationKind::Delete { target } => {
                self.tombstones.insert(target.clone());
            }
        }

        ApplyOutcome::Applied
    }

    /// Returns visible items in deterministic RGA order.
    #[must_use]
    pub fn visible_items(&self) -> Vec<RgaVisibleItem> {
        let mut children = BTreeMap::<Option<RgaId>, Vec<RgaId>>::new();

        for (id, node) in &self.nodes {
            children
                .entry(node.after.clone())
                .or_default()
                .push(id.clone());
        }

        for siblings in children.values_mut() {
            siblings.sort_by(|left, right| right.cmp(left));
        }

        let mut visible = Vec::new();
        let mut visited = BTreeSet::new();
        self.visit_children(None, &children, &mut visited, &mut visible);
        visible
    }

    /// Returns a deterministic visible-plus-metadata snapshot.
    #[must_use]
    pub fn snapshot(&self) -> RgaSnapshot {
        let nodes = self
            .nodes
            .iter()
            .map(|(id, node)| RgaNodeMetadata {
                node: id.clone(),
                after: node.after.clone(),
                item: node.item.clone(),
                value: node.value.clone(),
                tombstone: self.tombstones.contains(id),
            })
            .collect();

        RgaSnapshot {
            items: self.visible_items(),
            nodes,
        }
    }

    /// Serializes the full deterministic RGA snapshot as JSON.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if serde cannot encode the snapshot.
    pub fn snapshot_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.snapshot())
    }

    /// Returns explicit structural invariant violations.
    #[must_use]
    pub fn invariant_violations(&self) -> Vec<RgaInvariantViolation> {
        let mut violations = Vec::new();

        for (id, node) in &self.nodes {
            if let Some(predecessor) = &node.after
                && !self.nodes.contains_key(predecessor)
            {
                violations.push(RgaInvariantViolation::MissingPredecessor {
                    node: id.clone(),
                    predecessor: predecessor.clone(),
                });
            }

            if self.predecessor_chain_has_cycle(id) {
                violations.push(RgaInvariantViolation::Cycle { node: id.clone() });
            }
        }

        let mut by_item = BTreeMap::<ItemId, Vec<RgaId>>::new();
        for (id, node) in &self.nodes {
            by_item
                .entry(node.item.clone())
                .or_default()
                .push(id.clone());
        }

        for (item, nodes) in by_item {
            if nodes.len() > 1 {
                violations.push(RgaInvariantViolation::DuplicateItemIdentity { item, nodes });
            }
        }

        violations
    }

    /// Returns whether two replicas contain identical RGA state.
    ///
    /// Local replica identity and clock are excluded. Observed operations,
    /// predecessor metadata, values, and tombstones are included.
    #[must_use]
    pub fn equivalent_crdt_state(&self, other: &Self) -> bool {
        self.seen == other.seen && self.nodes == other.nodes && self.tombstones == other.tombstones
    }

    fn next_timestamp(&mut self) -> Result<Timestamp, RgaError> {
        self.clock = self.clock.checked_add(1).ok_or(RgaError::ClockExhausted)?;
        Ok(Timestamp {
            counter: self.clock,
            replica: self.replica.clone(),
        })
    }

    fn visit_children(
        &self,
        parent: Option<RgaId>,
        children: &BTreeMap<Option<RgaId>, Vec<RgaId>>,
        visited: &mut BTreeSet<RgaId>,
        visible: &mut Vec<RgaVisibleItem>,
    ) {
        let Some(nodes) = children.get(&parent) else {
            return;
        };

        for id in nodes {
            if !visited.insert(id.clone()) {
                continue;
            }

            if let Some(node) = self.nodes.get(id) {
                if !self.tombstones.contains(id) {
                    visible.push(RgaVisibleItem {
                        id: node.item.clone(),
                        value: node.value.clone(),
                        node: id.clone(),
                    });
                }
                self.visit_children(Some(id.clone()), children, visited, visible);
            }
        }
    }

    fn predecessor_chain_has_cycle(&self, start: &RgaId) -> bool {
        let mut seen = BTreeSet::new();
        let mut current = Some(start.clone());

        while let Some(id) = current {
            if !seen.insert(id.clone()) {
                return true;
            }
            current = self.nodes.get(&id).and_then(|node| node.after.clone());
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(value: &str) -> ItemId {
        ItemId::new(value)
    }

    fn replica(value: &str) -> RgaReplica {
        RgaReplica::new(ReplicaId::new(value))
    }

    fn deliver(target: &mut RgaReplica, operations: &[RgaOperation]) {
        for operation in operations {
            target.apply(operation);
        }
    }

    #[test]
    fn local_insertions_respect_visible_index() {
        let mut alice = replica("alice");
        alice.insert_at(0, item("a"), "A").unwrap();
        alice.insert_at(1, item("c"), "C").unwrap();
        alice.insert_at(1, item("b"), "B").unwrap();

        let ids = alice
            .visible_items()
            .into_iter()
            .map(|visible| visible.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec![item("a"), item("b"), item("c")]);
    }

    #[test]
    fn concurrent_sibling_insertions_converge_across_delivery_orders() {
        let mut source = replica("source");
        let insert_a = source.insert_at(0, item("a"), "A").unwrap();
        let insert_c = source.insert_at(1, item("c"), "C").unwrap();

        let initial = [insert_a.clone(), insert_c.clone()];
        let mut alice = replica("alice");
        let mut bob = replica("bob");
        deliver(&mut alice, &initial);
        deliver(&mut bob, &initial);

        let insert_b = alice.insert_at(1, item("b"), "B").unwrap();
        let insert_x = bob.insert_at(1, item("x"), "X").unwrap();

        let operations = [insert_a, insert_c, insert_b, insert_x];

        let mut forward = replica("forward");
        deliver(&mut forward, &operations);

        let mut reverse = replica("reverse");
        let reversed = operations.iter().cloned().rev().collect::<Vec<_>>();
        deliver(&mut reverse, &reversed);

        assert!(forward.equivalent_crdt_state(&reverse));
        assert_eq!(forward.snapshot(), reverse.snapshot());
        assert!(forward.invariant_violations().is_empty());
    }

    #[test]
    fn delete_delivered_before_insert_keeps_target_tombstoned() {
        let mut alice = replica("alice");
        let insert = alice.insert_at(0, item("a"), "A").unwrap();
        let delete = alice.delete(&item("a")).unwrap();

        let mut late = replica("late");
        late.apply(&delete);
        late.apply(&insert);

        assert!(late.visible_items().is_empty());
        assert!(late.invariant_violations().is_empty());
    }

    #[test]
    fn duplicate_delivery_is_explicitly_idempotent() {
        let mut alice = replica("alice");
        let operation = alice.insert_at(0, item("a"), "A").unwrap();
        let before = alice.snapshot();

        assert_eq!(alice.apply(&operation), ApplyOutcome::Duplicate);
        assert_eq!(alice.snapshot(), before);
    }

    #[test]
    fn snapshot_preserves_predecessors_and_tombstones() {
        let mut alice = replica("alice");
        alice.insert_at(0, item("a"), "A").unwrap();
        alice.insert_at(1, item("b"), "B").unwrap();
        alice.delete(&item("a")).unwrap();

        let snapshot = alice.snapshot();

        assert_eq!(snapshot.nodes.len(), 2);
        assert!(snapshot.nodes.iter().any(|node| node.tombstone));
        assert!(snapshot.nodes.iter().any(|node| node.after.is_some()));
    }
}
