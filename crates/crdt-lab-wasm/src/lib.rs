use crdt_lab::{ItemId, ListError, MovableListReplica, Operation, ReplicaId, Snapshot};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LabView {
    left: Snapshot,
    right: Snapshot,
    left_outbox: Vec<Operation>,
    right_outbox: Vec<Operation>,
    converged: bool,
}

#[wasm_bindgen]
pub struct TwoReplicaListLab {
    left: MovableListReplica,
    right: MovableListReplica,
    left_outbox: Vec<Operation>,
    right_outbox: Vec<Operation>,
}

#[wasm_bindgen]
impl TwoReplicaListLab {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Self, JsValue> {
        let mut left = MovableListReplica::new(ReplicaId::new("A"));
        let mut right = MovableListReplica::new(ReplicaId::new("B"));

        for (index, (id, value)) in [
            ("alpha", "Alpha"),
            ("bravo", "Bravo"),
            ("charlie", "Charlie"),
            ("delta", "Delta"),
        ]
        .into_iter()
        .enumerate()
        {
            let operation = left
                .insert_at(index, ItemId::new(id), value)
                .map_err(list_error)?;
            right.apply(&operation);
        }

        Ok(Self {
            left,
            right,
            left_outbox: Vec::new(),
            right_outbox: Vec::new(),
        })
    }

    pub fn move_item(
        &mut self,
        replica: &str,
        item: &str,
        target_index: usize,
    ) -> Result<(), JsValue> {
        let item = ItemId::new(item);
        let operation = match replica {
            "left" => self
                .left
                .move_to_index(&item, target_index)
                .map_err(list_error)?,
            "right" => self
                .right
                .move_to_index(&item, target_index)
                .map_err(list_error)?,
            _ => return Err(JsValue::from_str("unknown replica")),
        };

        match replica {
            "left" => self.left_outbox.push(operation),
            "right" => self.right_outbox.push(operation),
            _ => unreachable!(),
        }
        Ok(())
    }

    pub fn delete_item(&mut self, replica: &str, item: &str) -> Result<(), JsValue> {
        let item = ItemId::new(item);
        let operation = match replica {
            "left" => self.left.delete(&item).map_err(list_error)?,
            "right" => self.right.delete(&item).map_err(list_error)?,
            _ => return Err(JsValue::from_str("unknown replica")),
        };

        match replica {
            "left" => self.left_outbox.push(operation),
            "right" => self.right_outbox.push(operation),
            _ => unreachable!(),
        }
        Ok(())
    }

    pub fn deliver_next(&mut self, from: &str) -> Result<bool, JsValue> {
        match from {
            "left" => {
                if self.left_outbox.is_empty() {
                    return Ok(false);
                }
                let operation = self.left_outbox.remove(0);
                self.right.apply(&operation);
                Ok(true)
            }
            "right" => {
                if self.right_outbox.is_empty() {
                    return Ok(false);
                }
                let operation = self.right_outbox.remove(0);
                self.left.apply(&operation);
                Ok(true)
            }
            _ => Err(JsValue::from_str("unknown replica")),
        }
    }

    pub fn deliver_all(&mut self) {
        for operation in self.left_outbox.drain(..) {
            self.right.apply(&operation);
        }
        for operation in self.right_outbox.drain(..) {
            self.left.apply(&operation);
        }
    }

    pub fn view_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&LabView {
            left: self.left.snapshot(),
            right: self.right.snapshot(),
            left_outbox: self.left_outbox.clone(),
            right_outbox: self.right_outbox.clone(),
            converged: self.left.equivalent_crdt_state(&self.right),
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
    }
}

fn list_error(error: ListError) -> JsValue {
    JsValue::from_str(&format!("{error:?}"))
}
