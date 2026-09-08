//! Deterministic CRDT experiments.
//!
//! The first slice intentionally implements a small last-writer movable-list
//! baseline. It is useful because later algorithms can be compared against a
//! concrete, convergent model whose semantic compromises are explicit.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Stable identifier for a replica.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReplicaId(String);

impl ReplicaId {
    /// Creates a replica identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// Stable identifier for a list item.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ItemId(String);

impl ItemId {
    /// Creates an item identifier.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// Totally ordered Lamport timestamp.
///
/// The replica identifier breaks ties between concurrent operations with the
/// same counter. The resulting order is deterministic, not a claim about user
/// intent.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Timestamp {
    counter: u64,
    replica: ReplicaId,
}

impl Timestamp {
    /// Returns the Lamport counter.
    #[must_use]
    pub const fn counter(&self) -> u64 {
        self.counter
    }

    /// Returns the replica that created the timestamp.
    #[must_use]
    pub const fn replica(&self) -> &ReplicaId {
        &self.replica
    }
}

/// Dense lexicographic position used by the baseline list.
///
/// Positions are allocated between neighboring positions. Concurrent moves may
/// pick the same position, in which case the operation timestamp and item id
/// provide deterministic secondary ordering.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Position(Vec<u32>);

impl Position {
    /// Returns the position digits.
    #[must_use]
    pub fn digits(&self) -> &[u32] {
        &self.0
    }

    fn between(left: Option<&Self>, right: Option<&Self>) -> Self {
        let left = left.map_or(&[][..], |position| position.0.as_slice());
        let right = right.map_or(&[][..], |position| position.0.as_slice());
        let mut digits = Vec::new();

        for depth in 0.. {
            let left_digit = left.get(depth).copied().unwrap_or(0);
            let right_digit = right.get(depth).copied().unwrap_or(u32::MAX);

            debug_assert!(left_digit <= right_digit);

            let gap = right_digit - left_digit;
            if gap > 1 {
                digits.push(left_digit + gap / 2);
                return Self(digits);
            }

            digits.push(left_digit);
        }

        unreachable!("u32 position space always has another depth")
    }
}

/// A replicated operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Operation {
    timestamp: Timestamp,
    kind: OperationKind,
}

impl Operation {
    /// Returns the operation timestamp, which also acts as its stable id.
    #[must_use]
    pub const fn timestamp(&self) -> &Timestamp {
        &self.timestamp
    }

    /// Returns the operation payload.
    #[must_use]
    pub const fn kind(&self) -> &OperationKind {
        &self.kind
    }
}

/// Operation payload understood by the movable-list baseline.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OperationKind {
    /// Creates an item and assigns its initial position.
    Insert {
        /// Stable item identity.
        item: ItemId,
        /// Opaque value carried by the demonstration list.
        value: String,
        /// Initial dense position.
        position: Position,
    },
    /// Changes the position of an existing item while retaining its identity.
    Move {
        /// Stable item identity.
        item: ItemId,
        /// New dense position.
        position: Position,
    },
    /// Deletes an item according to last-writer semantics.
    Delete {
        /// Stable item identity.
        item: ItemId,
    },
}

/// Result of applying a delivered operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    /// The operation had not been observed before.
    Applied,
    /// The operation was a duplicate and changed no state.
    Duplicate,
}

/// Error returned while creating a local list operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListError {
    /// The requested index is outside the current visible list.
    IndexOutOfBounds {
        /// Requested insertion or move index.
        index: usize,
        /// Maximum accepted index.
        len: usize,
    },
    /// An insert reused an item identity already known as created locally.
    ItemAlreadyExists(ItemId),
    /// A move or delete targeted an item that is not currently visible.
    ItemNotVisible(ItemId),
    /// The local Lamport clock reached its maximum representable value.
    ClockExhausted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Versioned<T> {
    timestamp: Timestamp,
    value: T,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct ItemState {
    created: Option<Versioned<String>>,
    placement: Option<Versioned<Position>>,
    deleted_at: Option<Timestamp>,
}

/// One visible item in deterministic rendered order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VisibleItem {
    /// Stable item identity.
    pub id: ItemId,
    /// Current value selected by the latest insert for the identity.
    pub value: String,
    /// Current winning position.
    pub position: Position,
}

/// Deterministic visible snapshot of the movable list.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Snapshot {
    /// Visible items in rendered order.
    pub items: Vec<VisibleItem>,
}

/// Replica containing the first movable-list baseline.
///
/// Each logical field is a last-writer register ordered by [`Timestamp`]. This
/// makes operation application commutative and allows a move or delete to be
/// delivered before the corresponding insert without losing information.
#[derive(Clone, Debug)]
pub struct MovableListReplica {
    replica: ReplicaId,
    clock: u64,
    seen: BTreeSet<Timestamp>,
    items: BTreeMap<ItemId, ItemState>,
}

impl MovableListReplica {
    /// Creates an empty replica.
    #[must_use]
    pub fn new(replica: ReplicaId) -> Self {
        Self {
            replica,
            clock: 0,
            seen: BTreeSet::new(),
            items: BTreeMap::new(),
        }
    }

    /// Creates and locally applies an insert operation at `index`.
    ///
    /// # Errors
    ///
    /// Returns [`ListError::IndexOutOfBounds`] for an invalid index,
    /// [`ListError::ItemAlreadyExists`] when the item was already created, or
    /// [`ListError::ClockExhausted`] if no new timestamp can be allocated.
    pub fn insert_at(
        &mut self,
        index: usize,
        item: ItemId,
        value: impl Into<String>,
    ) -> Result<Operation, ListError> {
        if self
            .items
            .get(&item)
            .is_some_and(|state| state.created.is_some())
        {
            return Err(ListError::ItemAlreadyExists(item));
        }

        let position = self.position_for_index(index, None)?;
        let operation = Operation {
            timestamp: self.next_timestamp()?,
            kind: OperationKind::Insert {
                item,
                value: value.into(),
                position,
            },
        };
        let outcome = self.apply(&operation);
        debug_assert_eq!(outcome, ApplyOutcome::Applied);
        Ok(operation)
    }

    /// Creates and locally applies a move operation to the final visible index.
    ///
    /// The item is temporarily excluded while its new neighboring positions are
    /// selected, so index zero means "before every other visible item".
    ///
    /// # Errors
    ///
    /// Returns [`ListError::ItemNotVisible`] when the item cannot currently be
    /// moved, [`ListError::IndexOutOfBounds`] for an invalid target index, or
    /// [`ListError::ClockExhausted`] if no new timestamp can be allocated.
    pub fn move_to_index(
        &mut self,
        item: &ItemId,
        index: usize,
    ) -> Result<Operation, ListError> {
        if !self.is_visible(item) {
            return Err(ListError::ItemNotVisible(item.clone()));
        }

        let position = self.position_for_index(index, Some(item))?;
        let operation = Operation {
            timestamp: self.next_timestamp()?,
            kind: OperationKind::Move {
                item: item.clone(),
                position,
            },
        };
        let outcome = self.apply(&operation);
        debug_assert_eq!(outcome, ApplyOutcome::Applied);
        Ok(operation)
    }

    /// Creates and locally applies a delete operation.
    ///
    /// A later winning move may make the item visible again. That behavior is
    /// intentional for this LWW baseline and is a semantic compromise for later
    /// algorithms to compare against.
    ///
    /// # Errors
    ///
    /// Returns [`ListError::ItemNotVisible`] when the item is already absent or
    /// [`ListError::ClockExhausted`] if no new timestamp can be allocated.
    pub fn delete(&mut self, item: &ItemId) -> Result<Operation, ListError> {
        if !self.is_visible(item) {
            return Err(ListError::ItemNotVisible(item.clone()));
        }

        let operation = Operation {
            timestamp: self.next_timestamp()?,
            kind: OperationKind::Delete { item: item.clone() },
        };
        let outcome = self.apply(&operation);
        debug_assert_eq!(outcome, ApplyOutcome::Applied);
        Ok(operation)
    }

    /// Applies a local or remote operation.
    ///
    /// Delivery order does not determine the winning state. Each field retains
    /// the greatest timestamp it has observed, and duplicate operation ids are
    /// explicitly ignored.
    pub fn apply(&mut self, operation: &Operation) -> ApplyOutcome {
        if !self.seen.insert(operation.timestamp.clone()) {
            return ApplyOutcome::Duplicate;
        }

        self.clock = self.clock.max(operation.timestamp.counter);

        match &operation.kind {
            OperationKind::Insert {
                item,
                value,
                position,
            } => {
                let state = self.items.entry(item.clone()).or_default();
                update_if_newer(
                    &mut state.created,
                    Versioned {
                        timestamp: operation.timestamp.clone(),
                        value: value.clone(),
                    },
                );
                update_if_newer(
                    &mut state.placement,
                    Versioned {
                        timestamp: operation.timestamp.clone(),
                        value: position.clone(),
                    },
                );
            }
            OperationKind::Move { item, position } => {
                let state = self.items.entry(item.clone()).or_default();
                update_if_newer(
                    &mut state.placement,
                    Versioned {
                        timestamp: operation.timestamp.clone(),
                        value: position.clone(),
                    },
                );
            }
            OperationKind::Delete { item } => {
                let state = self.items.entry(item.clone()).or_default();
                if state
                    .deleted_at
                    .as_ref()
                    .is_none_or(|current| operation.timestamp > *current)
                {
                    state.deleted_at = Some(operation.timestamp.clone());
                }
            }
        }

        ApplyOutcome::Applied
    }

    /// Returns the visible items in deterministic order.
    #[must_use]
    pub fn visible_items(&self) -> Vec<VisibleItem> {
        let mut items = self
            .items
            .iter()
            .filter_map(|(id, state)| {
                let created = state.created.as_ref()?;
                let placement = state.placement.as_ref()?;

                if state
                    .deleted_at
                    .as_ref()
                    .is_some_and(|deleted_at| deleted_at >= &placement.timestamp)
                {
                    return None;
                }

                Some((
                    placement.value.clone(),
                    placement.timestamp.clone(),
                    id.clone(),
                    created.value.clone(),
                ))
            })
            .collect::<Vec<_>>();

        items.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });

        items
            .into_iter()
            .map(|(position, _timestamp, id, value)| VisibleItem {
                id,
                value,
                position,
            })
            .collect()
    }

    /// Returns a deterministic snapshot suitable for semantic assertions.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            items: self.visible_items(),
        }
    }

    /// Serializes the visible snapshot as deterministic JSON.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if serde cannot encode the snapshot.
    pub fn snapshot_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.snapshot())
    }

    /// Returns whether two replicas contain identical replicated state.
    ///
    /// Replica identity and the local clock are deliberately excluded. The set
    /// of observed operations and all per-item conflict metadata are included.
    #[must_use]
    pub fn equivalent_crdt_state(&self, other: &Self) -> bool {
        self.seen == other.seen && self.items == other.items
    }

    fn next_timestamp(&mut self) -> Result<Timestamp, ListError> {
        self.clock = self.clock.checked_add(1).ok_or(ListError::ClockExhausted)?;
        Ok(Timestamp {
            counter: self.clock,
            replica: self.replica.clone(),
        })
    }

    fn is_visible(&self, item: &ItemId) -> bool {
        self.visible_items().iter().any(|visible| &visible.id == item)
    }

    fn position_for_index(
        &self,
        index: usize,
        excluded: Option<&ItemId>,
    ) -> Result<Position, ListError> {
        let items = self
            .visible_items()
            .into_iter()
            .filter(|item| excluded != Some(&item.id))
            .collect::<Vec<_>>();

        if index > items.len() {
            return Err(ListError::IndexOutOfBounds {
                index,
                len: items.len(),
            });
        }

        let left = index.checked_sub(1).and_then(|left| items.get(left));
        let right = items.get(index);

        Ok(Position::between(
            left.map(|item| &item.position),
            right.map(|item| &item.position),
        ))
    }
}

fn update_if_newer<T>(slot: &mut Option<Versioned<T>>, candidate: Versioned<T>) {
    if slot
        .as_ref()
        .is_none_or(|current| candidate.timestamp > current.timestamp)
    {
        *slot = Some(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(value: &str) -> ItemId {
        ItemId::new(value)
    }

    fn replica(value: &str) -> MovableListReplica {
        MovableListReplica::new(ReplicaId::new(value))
    }

    fn create_initial_list(owner: &mut MovableListReplica) -> Vec<Operation> {
        vec![
            owner.insert_at(0, item("a"), "A").unwrap(),
            owner.insert_at(1, item("b"), "B").unwrap(),
            owner.insert_at(2, item("c"), "C").unwrap(),
        ]
    }

    fn deliver(replica: &mut MovableListReplica, operations: &[Operation]) {
        for operation in operations {
            replica.apply(operation);
        }
    }

    #[test]
    fn first_class_move_preserves_item_identity() {
        let mut alice = replica("alice");
        create_initial_list(&mut alice);

        alice.move_to_index(&item("c"), 1).unwrap();

        let ids = alice
            .visible_items()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![item("a"), item("c"), item("b")]);
    }

    #[test]
    fn replicas_converge_across_delivery_orders() {
        let mut source = replica("source");
        let initial = create_initial_list(&mut source);

        let mut alice = replica("alice");
        let mut bob = replica("bob");
        deliver(&mut alice, &initial);
        deliver(&mut bob, &initial);

        let alice_move = alice.move_to_index(&item("c"), 0).unwrap();
        let bob_move = bob.move_to_index(&item("a"), 2).unwrap();

        let mut operations = initial;
        operations.push(alice_move);
        operations.push(bob_move);

        let mut forward = replica("forward");
        deliver(&mut forward, &operations);

        let mut reverse = replica("reverse");
        let reversed = operations.iter().cloned().rev().collect::<Vec<_>>();
        deliver(&mut reverse, &reversed);

        let mut duplicated = replica("duplicated");
        let mixed = [
            operations[4].clone(),
            operations[1].clone(),
            operations[3].clone(),
            operations[0].clone(),
            operations[4].clone(),
            operations[2].clone(),
            operations[1].clone(),
        ];
        deliver(&mut duplicated, &mixed);

        assert!(forward.equivalent_crdt_state(&reverse));
        assert!(forward.equivalent_crdt_state(&duplicated));
        assert_eq!(forward.snapshot_json().unwrap(), reverse.snapshot_json().unwrap());
        assert_eq!(forward.snapshot_json().unwrap(), duplicated.snapshot_json().unwrap());
    }

    #[test]
    fn concurrent_moves_use_timestamp_and_replica_tie_breaking() {
        let mut source = replica("source");
        let initial = create_initial_list(&mut source);

        let mut alice = replica("alice");
        let mut bob = replica("bob");
        deliver(&mut alice, &initial);
        deliver(&mut bob, &initial);

        let alice_move = alice.move_to_index(&item("c"), 0).unwrap();
        let bob_move = bob.move_to_index(&item("c"), 2).unwrap();
        assert_eq!(alice_move.timestamp().counter(), bob_move.timestamp().counter());
        assert!(bob_move.timestamp() > alice_move.timestamp());

        let mut merged = replica("merged");
        deliver(&mut merged, &initial);
        merged.apply(&bob_move);
        merged.apply(&alice_move);

        let ids = merged
            .visible_items()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![item("a"), item("b"), item("c")]);
    }

    #[test]
    fn concurrent_delete_beats_move_when_its_timestamp_orders_later() {
        let mut source = replica("source");
        let initial = create_initial_list(&mut source);

        let mut alice = replica("alice");
        let mut bob = replica("bob");
        deliver(&mut alice, &initial);
        deliver(&mut bob, &initial);

        let alice_move = alice.move_to_index(&item("b"), 0).unwrap();
        let bob_delete = bob.delete(&item("b")).unwrap();
        assert!(bob_delete.timestamp() > alice_move.timestamp());

        let mut merged = replica("merged");
        deliver(&mut merged, &initial);
        merged.apply(&bob_delete);
        merged.apply(&alice_move);

        assert!(!merged
            .visible_items()
            .iter()
            .any(|visible| visible.id == item("b")));
    }

    #[test]
    fn move_delivered_before_insert_is_not_lost() {
        let mut source = replica("source");
        let initial = create_initial_list(&mut source);

        let mut bob = replica("bob");
        deliver(&mut bob, &initial);
        let move_b = bob.move_to_index(&item("b"), 0).unwrap();

        let mut late = replica("late");
        late.apply(&move_b);
        deliver(&mut late, &initial);

        let ids = late
            .visible_items()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![item("b"), item("a"), item("c")]);
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
    fn snapshot_json_is_deterministic() {
        let mut alice = replica("alice");
        alice.insert_at(0, item("a"), "A").unwrap();

        assert_eq!(
            alice.snapshot_json().unwrap(),
            r#"{"items":[{"id":"a","value":"A","position":[2147483647]}]}"#
        );
    }
}
