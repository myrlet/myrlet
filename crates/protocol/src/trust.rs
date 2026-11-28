//! Worker approval, applied scheduling inventory, and the two Hub Security & Trust
//! policies (`worker-protocol.md`).

use serde::{Deserialize, Serialize};

use crate::id::WorkerId;
use crate::inventory::{SlotInventory, WorkerInventory};

/// Effective trust state of one enrolled Worker.
///
/// A revoked credential is reported as `revoked`; otherwise the stored approval
/// decision is `pending` or `approved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerApprovalStatus {
    Pending,
    Approved,
    Revoked,
}

/// Server-controlled permission tier of a Worker principal (`security.md`).
///
/// Only these two tiers can be stored on a Worker; the Admin principal is a
/// separate identity authenticated by the Hub Admin token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionTier {
    /// Execute assigned Deliveries and read one-hop related-Task summaries.
    #[default]
    Worker,
    /// Global Task coordination plus read-only Worker/Agent observability.
    Owner,
}

impl PermissionTier {
    /// Canonical wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Owner => "owner",
        }
    }

    /// Parse the canonical wire token; anything else is refused.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "worker" => Some(Self::Worker),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }

    /// Whether this tier may coordinate any Task and read global control-plane state.
    pub fn is_global(self) -> bool {
        matches!(self, Self::Owner)
    }
}

/// `PATCH /v1/workers/{id}/permission-tier` request: the tier Admin is assigning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWorkerPermissionTierRequest {
    pub permission_tier: PermissionTier,
}

/// `PATCH /v1/workers/{id}/permission-tier` response: the stored tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerPermissionTierResponse {
    pub worker_id: WorkerId,
    pub permission_tier: PermissionTier,
}

/// The reviewable scheduling inventory the Hub keeps for one Worker revision.
///
/// Host observations and timestamps are ordinary telemetry and never part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerSchedulingConfig {
    #[serde(default)]
    pub slots: Vec<SlotInventory>,
}

impl WorkerSchedulingConfig {
    /// Project the reviewable content of one report, dropping host telemetry and timestamps.
    pub fn from_inventory(inventory: &WorkerInventory) -> Self {
        Self {
            slots: inventory.slots.clone(),
        }
    }
}

/// The two Hub-wide Security & Trust policies. Both default to on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecurityTrustSettings {
    /// A successful Pair leaves the Worker `pending` until an Admin approves it.
    pub require_worker_approval: bool,
    /// A first or changed scheduling inventory stays pending until an Admin applies it.
    pub require_configuration_apply: bool,
}

impl Default for SecurityTrustSettings {
    fn default() -> Self {
        Self {
            require_worker_approval: true,
            require_configuration_apply: true,
        }
    }
}

/// `PATCH /v1/settings/security-trust` request; omitted fields retain their values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSecurityTrustRequest {
    #[serde(default)]
    pub require_worker_approval: Option<bool>,
    #[serde(default)]
    pub require_configuration_apply: Option<bool>,
}

impl UpdateSecurityTrustRequest {
    /// Merge the supplied fields over the stored pair.
    pub fn merged_over(self, current: SecurityTrustSettings) -> SecurityTrustSettings {
        SecurityTrustSettings {
            require_worker_approval: self
                .require_worker_approval
                .unwrap_or(current.require_worker_approval),
            require_configuration_apply: self
                .require_configuration_apply
                .unwrap_or(current.require_configuration_apply),
        }
    }
}

/// `POST /v1/workers/{id}/config/apply` request: the revision the Admin reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyWorkerConfigRequest {
    pub revision: u64,
}

/// `POST /v1/workers/{id}/config/apply` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyWorkerConfigResponse {
    pub applied_revision: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn policies_default_on_and_merge_omitted_fields() {
        let current = SecurityTrustSettings::default();
        assert!(current.require_worker_approval && current.require_configuration_apply);
        let request: UpdateSecurityTrustRequest =
            serde_json::from_str(r#"{"requireConfigurationApply":false}"#).unwrap();
        let merged = request.merged_over(current);
        assert!(merged.require_worker_approval);
        assert!(!merged.require_configuration_apply);
    }

    #[test]
    fn approval_status_and_apply_dtos_use_wire_names() {
        assert_eq!(
            serde_json::to_value(WorkerApprovalStatus::Pending).unwrap(),
            "pending"
        );
        assert_eq!(
            serde_json::to_value(WorkerApprovalStatus::Revoked).unwrap(),
            "revoked"
        );
        let request: ApplyWorkerConfigRequest =
            serde_json::from_value(json!({ "revision": 4 })).unwrap();
        assert_eq!(request.revision, 4);
        assert_eq!(
            serde_json::to_value(ApplyWorkerConfigResponse {
                applied_revision: 4
            })
            .unwrap(),
            json!({ "appliedRevision": 4 })
        );
        assert!(serde_json::from_str::<UpdateSecurityTrustRequest>(
            r#"{"requireWorkerApproval":"yes"}"#
        )
        .is_err());
    }

    #[test]
    fn scheduling_config_rejects_unknown_fields_and_defaults_slots() {
        let empty: WorkerSchedulingConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.slots.is_empty());
        assert!(serde_json::from_str::<WorkerSchedulingConfig>(r#"{"extra":1}"#).is_err());
    }
}
