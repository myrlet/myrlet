//! Worker-reported inventory DTOs (`worker.hello` / `worker.inventory`).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::id::{ExecutorId, SlotId};

pub const MAX_INVENTORY_BYTES: usize = 256 * 1024;

pub const MAX_INVENTORY_SLOTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Native,
    Container,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostFacts {
    pub os: String,
    pub arch: String,
    pub deployment_mode: DeploymentMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityClaim {
    pub name: String,
    pub state: CapabilityState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutorInfo {
    pub id: ExecutorId,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotInventory {
    pub slot_id: SlotId,
    pub enabled: bool,
    /// Advisory only; `None` means unknown and is serialized as `null`.
    pub advisory_capacity: Option<u32>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityClaim>,
    #[serde(default)]
    pub executors: Vec<ExecutorInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerInventory {
    pub revision: u64,
    pub observed_at_ms: i64,
    pub host: HostFacts,
    #[serde(default)]
    pub slots: Vec<SlotInventory>,
}

impl WorkerInventory {
    /// Enforce V1 bounds: at most [`MAX_INVENTORY_SLOTS`] Slots with unique IDs.
    pub fn validate(&self) -> Result<(), InventoryValidationError> {
        if self.slots.len() > MAX_INVENTORY_SLOTS {
            return Err(InventoryValidationError::TooManySlots {
                reported: self.slots.len(),
                max: MAX_INVENTORY_SLOTS,
            });
        }
        let unique: HashSet<_> = self.slots.iter().map(|slot| &slot.slot_id).collect();
        if unique.len() == self.slots.len() {
            Ok(())
        } else {
            Err(InventoryValidationError::DuplicateSlotId)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InventoryValidationError {
    #[error("inventory reports {reported} slots, maximum is {max}")]
    TooManySlots { reported: usize, max: usize },
    #[error("inventory reports duplicate slot ids")]
    DuplicateSlotId,
}

impl SlotInventory {
    /// Whether this enabled Slot satisfies the hard routing rules of one Task.
    ///
    /// Required capabilities must all be `available` and an explicitly requested
    /// Executor must exist here. Worker, role, and other preferences never block a claim.
    pub fn accepts(
        &self,
        routing: &crate::task::TaskRouting,
        executor_id: Option<&ExecutorId>,
    ) -> bool {
        self.enabled
            && routing.required_capabilities.iter().all(|required| {
                self.capabilities.iter().any(|claim| {
                    claim.name == *required && claim.state == CapabilityState::Available
                })
            })
            && executor_id.is_none_or(|id| self.executors.iter().any(|exec| &exec.id == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::to_value;

    const MAX: usize = MAX_INVENTORY_SLOTS;

    fn slot(id: &str) -> SlotInventory {
        SlotInventory {
            slot_id: SlotId::new(id),
            enabled: true,
            advisory_capacity: None,
            roles: Vec::new(),
            capabilities: Vec::new(),
            executors: Vec::new(),
        }
    }

    fn inventory(slots: Vec<SlotInventory>) -> WorkerInventory {
        WorkerInventory {
            revision: 3,
            observed_at_ms: 0,
            host: HostFacts {
                os: "linux".into(),
                arch: "aarch64".into(),
                deployment_mode: DeploymentMode::Native,
            },
            slots,
        }
    }

    #[test]
    fn slot_wire_shape_is_camel_case_with_null_capacity() {
        let value = serde_json::to_value(slot("slot-a")).unwrap();
        assert!(value["advisoryCapacity"].is_null());
        assert_eq!(value["slotId"], "slot-a");
        assert_eq!(to_value(DeploymentMode::Native).unwrap(), "native");
        assert_eq!(to_value(CapabilityState::Degraded).unwrap(), "degraded");
    }

    #[test]
    fn validate_enforces_bounds_and_unique_slots() {
        let full = (0..MAX).map(|i| slot(&i.to_string())).collect();
        assert_eq!(inventory(full).validate(), Ok(()));
        let too_many = (0..=MAX).map(|i| slot(&i.to_string())).collect();
        assert!(inventory(too_many).validate().is_err());
        assert!(inventory(vec![slot("same"), slot("same")])
            .validate()
            .is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let raw = r#"{"revision":1,"observedAtMs":0,"host":{"os":"linux","arch":"x86_64","deploymentMode":"native"},"extra":true}"#;
        assert!(serde_json::from_str::<WorkerInventory>(raw).is_err());
    }
}
