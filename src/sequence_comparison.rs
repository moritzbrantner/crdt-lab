use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    ApplyOutcome, ItemId, LocalAction, ReplicaId, RgaError, RgaInvariantViolation, RgaOperation,
    RgaReplica, RgaSnapshot, Scenario, ScenarioFailure, ScenarioReport, ScenarioStep,
    SemanticAssertion,
};

/// Side-by-side replay of one semantic scenario against the baseline and RGA.
///
/// Each algorithm owns its own operation and metadata representation. The only
/// shared layer is the semantic scenario: local actions, network topology,
/// delivery order, and assertions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SequenceComparisonReport {
    /// Existing movable-list baseline replay.
    pub baseline: ScenarioReport,
    /// RGA replay of the same semantic history.
    pub rga: RgaScenarioReport,
}

impl SequenceComparisonReport {
    /// Serializes the comparison as deterministic JSON.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if serde cannot encode the report.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// Deterministic RGA replay result for a shared scenario.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaScenarioReport {
    /// Scenario name.
    pub name: String,
    /// Seed copied from the scenario.
    pub seed: Option<u64>,
    /// Whether every RGA-compatible step and assertion passed.
    pub passed: bool,
    /// Per-step RGA diagnostics.
    pub steps: Vec<RgaScenarioStepReport>,
    /// Final visible state, algorithm metadata, and invariant status.
    pub final_snapshots: Vec<RgaReplicaSnapshot>,
    /// First replay failure, when present.
    pub failure: Option<ScenarioFailure>,
}

impl RgaScenarioReport {
    /// Serializes RGA replay diagnostics as deterministic JSON.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if serde cannot encode the report.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// RGA replay diagnostics for one scenario step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaScenarioStepReport {
    /// Zero-based scenario step index.
    pub index: usize,
    /// Result of executing the step.
    pub outcome: RgaStepOutcome,
    /// RGA snapshots immediately after the step.
    pub snapshots: Vec<RgaReplicaSnapshot>,
}

/// Result of one RGA replay step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RgaStepOutcome {
    /// A local RGA operation was created and applied.
    LocalOperation {
        /// Scenario-local operation id.
        operation_id: String,
        /// Exact algorithm-specific RGA operation.
        operation: RgaOperation,
    },
    /// A network link changed state.
    LinkChanged {
        /// One endpoint.
        left: ReplicaId,
        /// The other endpoint.
        right: ReplicaId,
        /// New connectivity state.
        connected: bool,
    },
    /// A stored RGA operation was delivered.
    Delivered {
        /// Scenario-local operation id.
        operation_id: String,
        /// Replica that created the operation.
        from: ReplicaId,
        /// Delivery target.
        to: ReplicaId,
        /// Whether this target applied a new operation or saw a duplicate.
        outcome: ApplyOutcome,
    },
    /// A semantic assertion passed.
    AssertionPassed {
        /// Assertion that passed.
        assertion: SemanticAssertion,
    },
    /// Replay stopped at an invalid or unsupported step.
    Failed {
        /// Deterministic diagnostic.
        message: String,
    },
}

/// RGA state for one named replica.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RgaReplicaSnapshot {
    /// Replica identity.
    pub replica: ReplicaId,
    /// Visible sequence plus RGA predecessor/tombstone metadata.
    pub snapshot: RgaSnapshot,
    /// Structural invariant violations, reported separately from convergence.
    pub invariant_violations: Vec<RgaInvariantViolation>,
}

#[derive(Clone)]
struct StoredOperation {
    source: ReplicaId,
    operation: RgaOperation,
}

struct RgaRuntime {
    replicas: BTreeMap<ReplicaId, RgaReplica>,
    operations: BTreeMap<String, StoredOperation>,
    links: BTreeMap<(ReplicaId, ReplicaId), bool>,
}

impl Scenario {
    /// Replays this exact semantic history using RGA sequence semantics.
    ///
    /// First-class move actions are intentionally unsupported because RGA does
    /// not natively model move while preserving identity. The comparison layer
    /// does not silently translate a move into delete-plus-insert.
    #[must_use]
    pub fn run_rga(&self) -> RgaScenarioReport {
        let mut runtime = match RgaRuntime::new(&self.replicas) {
            Ok(runtime) => runtime,
            Err(message) => {
                return RgaScenarioReport {
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
                Ok(outcome) => reports.push(RgaScenarioStepReport {
                    index,
                    outcome,
                    snapshots: runtime.snapshots(),
                }),
                Err(message) => {
                    reports.push(RgaScenarioStepReport {
                        index,
                        outcome: RgaStepOutcome::Failed {
                            message: message.clone(),
                        },
                        snapshots: runtime.snapshots(),
                    });

                    return RgaScenarioReport {
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

        RgaScenarioReport {
            name: self.name.clone(),
            seed: self.seed,
            passed: true,
            steps: reports,
            final_snapshots: runtime.snapshots(),
            failure: None,
        }
    }

    /// Runs the existing baseline and RGA against the same semantic history.
    #[must_use]
    pub fn compare_sequence_algorithms(&self) -> SequenceComparisonReport {
        SequenceComparisonReport {
            baseline: self.run(),
            rga: self.run_rga(),
        }
    }
}

impl RgaRuntime {
    fn new(replica_ids: &[ReplicaId]) -> Result<Self, String> {
        if replica_ids.is_empty() {
            return Err("scenario must declare at least one replica".to_owned());
        }

        let mut replicas = BTreeMap::new();
        for replica in replica_ids {
            if replicas
                .insert(replica.clone(), RgaReplica::new(replica.clone()))
                .is_some()
            {
                return Err(format!("duplicate replica '{}'", replica.as_str()));
            }
        }

        Ok(Self {
            replicas,
            operations: BTreeMap::new(),
            links: BTreeMap::new(),
        })
    }

    fn execute(&mut self, step: &ScenarioStep) -> Result<RgaStepOutcome, String> {
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
                self.links.insert(link_key(left, right), *connected);

                Ok(RgaStepOutcome::LinkChanged {
                    left: left.clone(),
                    right: right.clone(),
                    connected: *connected,
                })
            }
            ScenarioStep::Deliver { operation, to } => self.deliver(operation, to),
            ScenarioStep::Assert { assertion } => {
                self.check_assertion(assertion)?;
                Ok(RgaStepOutcome::AssertionPassed {
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
    ) -> Result<RgaStepOutcome, String> {
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
                .map_err(rga_error)?,
            LocalAction::Delete { item } => state.delete(item).map_err(rga_error)?,
            LocalAction::Move { .. } => {
                return Err(
                    "RGA does not support first-class move; comparison refuses to translate move into delete-plus-insert"
                        .to_owned(),
                );
            }
        };

        self.operations.insert(
            id.to_owned(),
            StoredOperation {
                source: replica.clone(),
                operation: operation.clone(),
            },
        );

        Ok(RgaStepOutcome::LocalOperation {
            operation_id: id.to_owned(),
            operation,
        })
    }

    fn deliver(&mut self, operation_id: &str, to: &ReplicaId) -> Result<RgaStepOutcome, String> {
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

        Ok(RgaStepOutcome::Delivered {
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
                    return Err("equivalent assertion requires at least two replicas".to_owned());
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
                            "replicas '{}' and '{}' do not contain equivalent RGA state",
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
                        "replica '{}' RGA visible order mismatch: expected {:?}, got {:?}",
                        replica.as_str(),
                        items,
                        actual
                    ))
                }
            }
        }
    }

    fn snapshots(&self) -> Vec<RgaReplicaSnapshot> {
        self.replicas
            .iter()
            .map(|(replica, state)| RgaReplicaSnapshot {
                replica: replica.clone(),
                snapshot: state.snapshot(),
                invariant_violations: state.invariant_violations(),
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

fn rga_error(error: RgaError) -> String {
    format!("{error:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONCURRENT_INSERT: &str = include_str!("../scenarios/concurrent-insert.json");
    const CONCURRENT_MOVE: &str = include_str!("../scenarios/concurrent-move.json");

    fn ids(items: impl IntoIterator<Item = ItemId>) -> Vec<String> {
        items
            .into_iter()
            .map(|item| item.as_str().to_owned())
            .collect()
    }

    #[test]
    fn same_history_converges_but_exposes_algorithm_specific_order() {
        let scenario = Scenario::from_json(CONCURRENT_INSERT).unwrap();
        let comparison = scenario.compare_sequence_algorithms();

        assert!(comparison.baseline.passed);
        assert!(comparison.rga.passed);

        let baseline = &comparison.baseline.final_snapshots[0].snapshot;
        let rga = &comparison.rga.final_snapshots[0];

        assert_eq!(
            ids(baseline.items.iter().map(|item| item.id.clone())),
            ["a", "b", "x", "c"]
        );
        assert_eq!(
            ids(rga.snapshot.items.iter().map(|item| item.id.clone())),
            ["a", "x", "b", "c"]
        );
        assert!(rga.invariant_violations.is_empty());
        assert!(rga.snapshot.nodes.iter().all(|node| !node.tombstone));
    }

    #[test]
    fn rga_replay_refuses_to_reinterpret_first_class_moves() {
        let scenario = Scenario::from_json(CONCURRENT_MOVE).unwrap();
        let report = scenario.run_rga();

        assert!(!report.passed);
        assert!(
            report
                .failure
                .as_ref()
                .unwrap()
                .message
                .contains("first-class move")
        );
    }

    #[test]
    fn comparison_json_preserves_both_algorithm_snapshots() {
        let scenario = Scenario::from_json(CONCURRENT_INSERT).unwrap();
        let comparison = scenario.compare_sequence_algorithms();
        let json = comparison.to_json().unwrap();

        assert!(json.contains("\"baseline\""));
        assert!(json.contains("\"rga\""));
        assert!(json.contains("\"nodes\""));
    }
}
