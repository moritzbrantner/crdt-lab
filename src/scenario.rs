use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ApplyOutcome, ItemId, ListError, MovableListReplica, Operation, ReplicaId, Snapshot,
};

/// Serializable deterministic experiment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Scenario {
    /// Human-readable scenario name.
    pub name: String,
    /// Optional seed that produced a generated scenario.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// Replicas participating in the experiment.
    pub replicas: Vec<ReplicaId>,
    /// Ordered local, network, and assertion steps.
    pub steps: Vec<ScenarioStep>,
}

impl Scenario {
    /// Parses a scenario from JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when the JSON does not match the scenario format.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serializes the scenario as stable, human-readable JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Replays the scenario and records deterministic diagnostics after every step.
    #[must_use]
    pub fn run(&self) -> ScenarioReport {
        let mut runtime = match Runtime::new(&self.replicas) {
            Ok(runtime) => runtime,
            Err(message) => {
                return ScenarioReport {
                    name: self.name.clone(),
                    seed: self.seed,
                    passed: false,
                    steps: Vec::new(),
                    final_snapshots: Vec::new(),
                    failure: Some(ScenarioFailure {
                        step_index: None,
                        message,
                    }),
                };
            }
        };

        let mut reports = Vec::with_capacity(self.steps.len());

        for (index, step) in self.steps.iter().enumerate() {
            match runtime.execute(step) {
                Ok(outcome) => reports.push(ScenarioStepReport {
                    index,
                    outcome,
                    snapshots: runtime.snapshots(),
                }),
                Err(message) => {
                    reports.push(ScenarioStepReport {
                        index,
                        outcome: StepOutcome::Failed {
                            message: message.clone(),
                        },
                        snapshots: runtime.snapshots(),
                    });

                    return ScenarioReport {
                        name: self.name.clone(),
                        seed: self.seed,
                        passed: false,
                        steps: reports,
                        final_snapshots: runtime.snapshots(),
                        failure: Some(ScenarioFailure {
                            step_index: Some(index),
                            message,
                        }),
                    };
                }
            }
        }

        ScenarioReport {
            name: self.name.clone(),
            seed: self.seed,
            passed: true,
            steps: reports,
            final_snapshots: runtime.snapshots(),
            failure: None,
        }
    }

    /// Builds a deterministic two-replica move history from a seed.
    ///
    /// The generated history starts from four shared items, partitions the two
    /// replicas, performs local first-class moves, reconnects the replicas,
    /// delivers the operations in a seed-derived order, and finally asserts
    /// equivalent replicated state.
    #[must_use]
    pub fn seeded_move_history(seed: u64, move_count: usize) -> Self {
        let alice = ReplicaId::new("alice");
        let bob = ReplicaId::new("bob");
        let mut steps = Vec::new();

        for (index, item) in ["alpha", "bravo", "charlie", "delta"]
            .into_iter()
            .enumerate()
        {
            let operation_id = format!("initial-{item}");
            steps.push(ScenarioStep::Local {
                id: operation_id.clone(),
                replica: alice.clone(),
                action: LocalAction::Insert {
                    index,
                    item: ItemId::new(item),
                    value: capitalize(item),
                },
            });
            steps.push(ScenarioStep::Deliver {
                operation: operation_id,
                to: bob.clone(),
            });
        }

        steps.push(ScenarioStep::Link {
            left: alice.clone(),
            right: bob.clone(),
            connected: false,
        });

        let mut rng = DeterministicRng::new(seed);
        let items = ["alpha", "bravo", "charlie", "delta"];
        let mut deliveries = Vec::with_capacity(move_count);

        for move_index in 0..move_count {
            let from_alice = rng.next_index(2) == 0;
            let replica = if from_alice {
                alice.clone()
            } else {
                bob.clone()
            };
            let target = if from_alice {
                bob.clone()
            } else {
                alice.clone()
            };
            let item = items[rng.next_index(items.len())];
            let target_index = rng.next_index(items.len());
            let operation_id = format!("move-{move_index}");

            steps.push(ScenarioStep::Local {
                id: operation_id.clone(),
                replica,
                action: LocalAction::Move {
                    item: ItemId::new(item),
                    target_index,
                },
            });
            deliveries.push((operation_id, target));
        }

        steps.push(ScenarioStep::Link {
            left: alice.clone(),
            right: bob.clone(),
            connected: true,
        });

        for index in (1..deliveries.len()).rev() {
            let swap_index = rng.next_index(index + 1);
            deliveries.swap(index, swap_index);
        }

        for (operation, to) in deliveries {
            steps.push(ScenarioStep::Deliver { operation, to });
        }

        steps.push(ScenarioStep::Assert {
            assertion: SemanticAssertion::Equivalent {
                replicas: vec![alice.clone(), bob.clone()],
            },
        });

        Self {
            name: format!("seeded move history {seed}"),
            seed: Some(seed),
            replicas: vec![alice, bob],
            steps,
        }
    }
}

/// One deterministic step in a scenario.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScenarioStep {
    /// Creates an operation on one replica and stores it under a scenario-local id.
    Local {
        /// Stable scenario-local operation id used by delivery steps.
        id: String,
        /// Replica that creates the operation.
        replica: ReplicaId,
        /// Local user action.
        action: LocalAction,
    },
    /// Changes whether two replicas can exchange operations.
    Link {
        /// One endpoint.
        left: ReplicaId,
        /// The other endpoint.
        right: ReplicaId,
        /// Whether delivery is currently allowed.
        connected: bool,
    },
    /// Delivers a previously created operation to another replica.
    ///
    /// Repeating the same delivery is an explicit duplicate delivery. Delivering
    /// operations in a different order models reordering.
    Deliver {
        /// Scenario-local operation id.
        operation: String,
        /// Target replica.
        to: ReplicaId,
    },
    /// Checks a semantic or convergence expectation at this point in replay.
    Assert {
        /// Assertion to evaluate.
        assertion: SemanticAssertion,
    },
}

/// Local user action translated into the baseline CRDT operation model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocalAction {
    /// Inserts a new stable item.
    Insert {
        /// Visible insertion index.
        index: usize,
        /// Stable item identity.
        item: ItemId,
        /// Opaque demonstration value.
        value: String,
    },
    /// Moves an existing stable item.
    Move {
        /// Stable item identity.
        item: ItemId,
        /// Final visible index after excluding the moved item.
        target_index: usize,
    },
    /// Deletes an existing visible item.
    Delete {
        /// Stable item identity.
        item: ItemId,
    },
}

/// Assertion evaluated by the deterministic scenario runner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SemanticAssertion {
    /// Requires the listed replicas to contain identical replicated state.
    Equivalent {
        /// Replicas that must agree.
        replicas: Vec<ReplicaId>,
    },
    /// Requires a replica to expose exactly this stable item order.
    VisibleOrder {
        /// Replica whose visible order is checked.
        replica: ReplicaId,
        /// Expected visible stable identities.
        items: Vec<ItemId>,
    },
}

/// Deterministic replay result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScenarioReport {
    /// Scenario name.
    pub name: String,
    /// Seed copied from the scenario.
    pub seed: Option<u64>,
    /// Whether every step and assertion completed successfully.
    pub passed: bool,
    /// Per-step diagnostics.
    pub steps: Vec<ScenarioStepReport>,
    /// Final visible state for every replica.
    pub final_snapshots: Vec<ReplicaSnapshot>,
    /// First replay failure, when present.
    pub failure: Option<ScenarioFailure>,
}

impl ScenarioReport {
    /// Serializes replay diagnostics as stable, human-readable JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Replay diagnostics for one scenario step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScenarioStepReport {
    /// Zero-based scenario step index.
    pub index: usize,
    /// Result of executing the step.
    pub outcome: StepOutcome,
    /// Visible snapshots immediately after the step.
    pub snapshots: Vec<ReplicaSnapshot>,
}

/// Result of one replay step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepOutcome {
    /// A local operation was created and applied.
    LocalOperation {
        /// Scenario-local operation id.
        operation_id: String,
        /// Exact CRDT operation produced by the local action.
        operation: Operation,
    },
    /// A network link changed state.
    LinkChanged {
        /// One endpoint.
        left: ReplicaId,
        /// The other endpoint.
        right: ReplicaId,
        /// New link state.
        connected: bool,
    },
    /// An operation was delivered.
    Delivered {
        /// Scenario-local operation id.
        operation_id: String,
        /// Source replica that originally created the operation.
        from: ReplicaId,
        /// Delivery target.
        to: ReplicaId,
        /// Whether the target applied a new operation or observed a duplicate.
        outcome: ApplyOutcome,
    },
    /// A semantic assertion passed.
    AssertionPassed {
        /// Assertion that passed.
        assertion: SemanticAssertion,
    },
    /// Replay stopped because this step was invalid or an assertion failed.
    Failed {
        /// Deterministic diagnostic message.
        message: String,
    },
}

/// Visible state of one named replica.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplicaSnapshot {
    /// Replica identity.
    pub replica: ReplicaId,
    /// Visible CRDT snapshot.
    pub snapshot: Snapshot,
}

/// First failure encountered during replay.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScenarioFailure {
    /// Zero-based failing step, or none for invalid scenario setup.
    pub step_index: Option<usize>,
    /// Deterministic diagnostic message.
    pub message: String,
}

#[derive(Clone)]
struct StoredOperation {
    source: ReplicaId,
    operation: Operation,
}

struct Runtime {
    replicas: BTreeMap<ReplicaId, MovableListReplica>,
    operations: BTreeMap<String, StoredOperation>,
    links: BTreeMap<(ReplicaId, ReplicaId), bool>,
}

impl Runtime {
    fn new(replica_ids: &[ReplicaId]) -> Result<Self, String> {
        if replica_ids.is_empty() {
            return Err("scenario must declare at least one replica".to_owned());
        }

        let mut seen = BTreeSet::new();
        let mut replicas = BTreeMap::new();

        for replica in replica_ids {
            if !seen.insert(replica.clone()) {
                return Err(format!("duplicate replica '{}'", replica.as_str()));
            }
            replicas.insert(
                replica.clone(),
                MovableListReplica::new(replica.clone()),
            );
        }

        Ok(Self {
            replicas,
            operations: BTreeMap::new(),
            links: BTreeMap::new(),
        })
    }

    fn execute(&mut self, step: &ScenarioStep) -> Result<StepOutcome, String> {
        match step {
            ScenarioStep::Local {
                id,
                replica,
                action,
            } => self.execute_local(id, replica, action),
            ScenarioStep::Link {
                left,
                right,
                connected,
            } => {
                self.ensure_replica(left)?;
                self.ensure_replica(right)?;
                if left == right {
                    return Err("a network link requires two different replicas".to_owned());
                }
                self.links
                    .insert(link_key(left, right), *connected);
                Ok(StepOutcome::LinkChanged {
                    left: left.clone(),
                    right: right.clone(),
                    connected: *connected,
                })
            }
            ScenarioStep::Deliver { operation, to } => self.deliver(operation, to),
            ScenarioStep::Assert { assertion } => {
                self.check_assertion(assertion)?;
                Ok(StepOutcome::AssertionPassed {
                    assertion: assertion.clone(),
                })
            }
        }
    }

    fn execute_local(
        &mut self,
        id: &str,
        replica: &ReplicaId,
        action: &LocalAction,
    ) -> Result<StepOutcome, String> {
        if self.operations.contains_key(id) {
            return Err(format!("operation id '{id}' is already defined"));
        }

        let state = self
            .replicas
            .get_mut(replica)
            .ok_or_else(|| unknown_replica(replica))?;

        let operation = match action {
            LocalAction::Insert {
                index,
                item,
                value,
            } => state
                .insert_at(*index, item.clone(), value.clone())
                .map_err(list_error)?,
            LocalAction::Move { item, target_index } => state
                .move_to_index(item, *target_index)
                .map_err(list_error)?,
            LocalAction::Delete { item } => state.delete(item).map_err(list_error)?,
        };

        self.operations.insert(
            id.to_owned(),
            StoredOperation {
                source: replica.clone(),
                operation: operation.clone(),
            },
        );

        Ok(StepOutcome::LocalOperation {
            operation_id: id.to_owned(),
            operation,
        })
    }

    fn deliver(&mut self, operation_id: &str, to: &ReplicaId) -> Result<StepOutcome, String> {
        self.ensure_replica(to)?;

        let stored = self
            .operations
            .get(operation_id)
            .cloned()
            .ok_or_else(|| format!("unknown operation id '{operation_id}'"))?;

        if stored.source == *to {
            return Err(format!(
                "operation '{operation_id}' is already local to replica '{}'",
                to.as_str()
            ));
        }

        if !self.is_connected(&stored.source, to) {
            return Err(format!(
                "cannot deliver operation '{operation_id}' from '{}' to '{}': link is partitioned",
                stored.source.as_str(),
                to.as_str()
            ));
        }

        let target = self
            .replicas
            .get_mut(to)
            .ok_or_else(|| unknown_replica(to))?;
        let outcome = target.apply(&stored.operation);

        Ok(StepOutcome::Delivered {
            operation_id: operation_id.to_owned(),
            from: stored.source,
            to: to.clone(),
            outcome,
        })
    }

    fn check_assertion(&self, assertion: &SemanticAssertion) -> Result<(), String> {
        match assertion {
            SemanticAssertion::Equivalent { replicas } => {
                if replicas.len() < 2 {
                    return Err(
                        "equivalent assertion requires at least two replicas".to_owned(),
                    );
                }

                let first_id = &replicas[0];
                let first = self
                    .replicas
                    .get(first_id)
                    .ok_or_else(|| unknown_replica(first_id))?;

                for replica_id in &replicas[1..] {
                    let replica = self
                        .replicas
                        .get(replica_id)
                        .ok_or_else(|| unknown_replica(replica_id))?;
                    if !first.equivalent_crdt_state(replica) {
                        return Err(format!(
                            "replicas '{}' and '{}' do not contain equivalent CRDT state",
                            first_id.as_str(),
                            replica_id.as_str()
                        ));
                    }
                }

                Ok(())
            }
            SemanticAssertion::VisibleOrder { replica, items } => {
                let state = self
                    .replicas
                    .get(replica)
                    .ok_or_else(|| unknown_replica(replica))?;
                let actual = state
                    .visible_items()
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>();

                if actual == *items {
                    Ok(())
                } else {
                    Err(format!(
                        "replica '{}' visible order mismatch: expected {:?}, got {:?}",
                        replica.as_str(),
                        items,
                        actual
                    ))
                }
            }
        }
    }

    fn snapshots(&self) -> Vec<ReplicaSnapshot> {
        self.replicas
            .iter()
            .map(|(replica, state)| ReplicaSnapshot {
                replica: replica.clone(),
                snapshot: state.snapshot(),
            })
            .collect()
    }

    fn ensure_replica(&self, replica: &ReplicaId) -> Result<(), String> {
        if self.replicas.contains_key(replica) {
            Ok(())
        } else {
            Err(unknown_replica(replica))
        }
    }

    fn is_connected(&self, left: &ReplicaId, right: &ReplicaId) -> bool {
        self.links
            .get(&link_key(left, right))
            .copied()
            .unwrap_or(true)
    }
}

fn link_key(left: &ReplicaId, right: &ReplicaId) -> (ReplicaId, ReplicaId) {
    if left <= right {
        (left.clone(), right.clone())
    } else {
        (right.clone(), left.clone())
    }
}

fn unknown_replica(replica: &ReplicaId) -> String {
    format!("unknown replica '{}'", replica.as_str())
}

fn list_error(error: ListError) -> String {
    format!("{error:?}")
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    fn next_index(&mut self, upper_exclusive: usize) -> usize {
        debug_assert!(upper_exclusive > 0);
        let upper = u64::try_from(upper_exclusive).expect("usize must fit into u64");
        usize::try_from(self.next_u64() % upper).expect("bounded random index must fit usize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONCURRENT_MOVE: &str = include_str!("../scenarios/concurrent-move.json");
    const MOVE_DELETE: &str = include_str!("../scenarios/move-delete.json");
    const REORDERED_DUPLICATE: &str =
        include_str!("../scenarios/reordered-duplicate-delivery.json");

    #[test]
    fn curated_scenarios_parse_and_pass() {
        for json in [CONCURRENT_MOVE, MOVE_DELETE, REORDERED_DUPLICATE] {
            let scenario = Scenario::from_json(json).unwrap();
            let report = scenario.run();
            assert!(report.passed, "{:?}", report.failure);
        }
    }

    #[test]
    fn duplicate_delivery_is_visible_in_replay_diagnostics() {
        let scenario = Scenario::from_json(REORDERED_DUPLICATE).unwrap();
        let report = scenario.run();

        assert!(report.steps.iter().any(|step| {
            matches!(
                step.outcome,
                StepOutcome::Delivered {
                    outcome: ApplyOutcome::Duplicate,
                    ..
                }
            )
        }));
    }

    #[test]
    fn generated_move_histories_are_reproducible_and_convergent() {
        let first = Scenario::seeded_move_history(42, 32);
        let second = Scenario::seeded_move_history(42, 32);

        assert_eq!(first, second);
        assert_eq!(first.to_json().unwrap(), second.to_json().unwrap());
        assert!(first.run().passed);
    }

    #[test]
    fn partitioned_delivery_fails_with_step_diagnostics() {
        let scenario = Scenario {
            name: "blocked delivery".to_owned(),
            seed: None,
            replicas: vec![ReplicaId::new("alice"), ReplicaId::new("bob")],
            steps: vec![
                ScenarioStep::Local {
                    id: "insert-a".to_owned(),
                    replica: ReplicaId::new("alice"),
                    action: LocalAction::Insert {
                        index: 0,
                        item: ItemId::new("a"),
                        value: "A".to_owned(),
                    },
                },
                ScenarioStep::Link {
                    left: ReplicaId::new("alice"),
                    right: ReplicaId::new("bob"),
                    connected: false,
                },
                ScenarioStep::Deliver {
                    operation: "insert-a".to_owned(),
                    to: ReplicaId::new("bob"),
                },
            ],
        };

        let report = scenario.run();

        assert!(!report.passed);
        assert_eq!(report.failure.as_ref().unwrap().step_index, Some(2));
        assert!(
            report
                .failure
                .as_ref()
                .unwrap()
                .message
                .contains("partitioned")
        );
    }

    #[test]
    fn scenario_json_round_trip_is_deterministic() {
        let scenario = Scenario::from_json(CONCURRENT_MOVE).unwrap();
        let encoded = scenario.to_json().unwrap();
        let decoded = Scenario::from_json(&encoded).unwrap();

        assert_eq!(decoded, scenario);
        assert_eq!(decoded.run(), scenario.run());
    }
}
