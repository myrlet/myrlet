//! Worker enrollment and fleet administration over any [`Transport`].

use myrlet_protocol::{
    ApplyWorkerConfigRequest, ApplyWorkerConfigResponse, CreateEnrollmentCodeRequest,
    CreateEnrollmentCodeResponse, PermissionTier, RotateWorkerCredentialResponse,
    SecurityTrustSettings, UpdateSecurityTrustRequest, UpdateWorkerPermissionTierRequest,
    WorkerDetail, WorkerId, WorkerListResponse, WorkerPermissionTierResponse,
};

use crate::error::ClientError;
use crate::ops::{decode_direct, ensure_path_segment, Client};
use crate::transport::Transport;
use crate::wire::encode_json;

impl<T: Transport> Client<T> {
    /// Issue a one-time Worker pairing code (`POST /v1/enrollment-codes`).
    pub async fn create_enrollment_code(
        &self,
        req: &CreateEnrollmentCodeRequest,
    ) -> Result<CreateEnrollmentCodeResponse, ClientError> {
        let body = self
            .post("/v1/enrollment-codes", None, encode_json(req)?)
            .await?;
        decode_direct(&body)
    }

    /// List enrolled Workers with connectivity and inventory (`GET /v1/workers`).
    pub async fn list_workers(&self) -> Result<WorkerListResponse, ClientError> {
        decode_direct(&self.get("/v1/workers").await?)
    }

    /// Read one Worker's detail, inventory, and claimable Task IDs (`GET /v1/workers/{id}`).
    pub async fn get_worker(&self, id: &WorkerId) -> Result<WorkerDetail, ClientError> {
        ensure_path_segment(id.as_str())?;
        decode_direct(&self.get(&format!("/v1/workers/{id}")).await?)
    }

    /// Rotate a Worker credential, fencing its WSS session (`POST /v1/workers/{id}/rotate`).
    pub async fn rotate_worker(
        &self,
        id: &WorkerId,
    ) -> Result<RotateWorkerCredentialResponse, ClientError> {
        ensure_path_segment(id.as_str())?;
        let body = self
            .exchange(
                "POST",
                &format!("/v1/workers/{id}/rotate"),
                true,
                None,
                None,
            )
            .await?;
        decode_direct(&body)
    }

    /// Revoke a Worker credential and disconnect it (`POST /v1/workers/{id}/revoke`).
    pub async fn revoke_worker(&self, id: &WorkerId) -> Result<(), ClientError> {
        ensure_path_segment(id.as_str())?;
        self.exchange(
            "POST",
            &format!("/v1/workers/{id}/revoke"),
            true,
            None,
            None,
        )
        .await
        .map(|_| ())
    }

    /// Approve a pending Worker (`POST /v1/workers/{id}/approve`).
    pub async fn approve_worker(&self, id: &WorkerId) -> Result<(), ClientError> {
        ensure_path_segment(id.as_str())?;
        self.exchange(
            "POST",
            &format!("/v1/workers/{id}/approve"),
            true,
            None,
            None,
        )
        .await
        .map(|_| ())
    }

    /// Apply a Worker's reported scheduling revision (`POST /v1/workers/{id}/config/apply`).
    pub async fn apply_worker_config(
        &self,
        id: &WorkerId,
        revision: u64,
    ) -> Result<ApplyWorkerConfigResponse, ClientError> {
        ensure_path_segment(id.as_str())?;
        let body = encode_json(&ApplyWorkerConfigRequest { revision })?;
        let response = self
            .post(&format!("/v1/workers/{id}/config/apply"), None, body)
            .await?;
        decode_direct(&response)
    }

    /// Set a Worker's server-controlled permission tier (`PATCH .../permission-tier`).
    pub async fn set_worker_permission_tier(
        &self,
        id: &WorkerId,
        tier: PermissionTier,
    ) -> Result<WorkerPermissionTierResponse, ClientError> {
        ensure_path_segment(id.as_str())?;
        let body = encode_json(&UpdateWorkerPermissionTierRequest {
            permission_tier: tier,
        })?;
        let response = self
            .exchange(
                "PATCH",
                &format!("/v1/workers/{id}/permission-tier"),
                true,
                None,
                Some(body),
            )
            .await?;
        decode_direct(&response)
    }

    /// Read the two Hub Security & Trust policies (`GET /v1/settings/security-trust`).
    pub async fn security_trust(&self) -> Result<SecurityTrustSettings, ClientError> {
        decode_direct(&self.get("/v1/settings/security-trust").await?)
    }

    /// Update one or both policies and return the stored pair (`PATCH /v1/settings/security-trust`).
    pub async fn update_security_trust(
        &self,
        request: &UpdateSecurityTrustRequest,
    ) -> Result<SecurityTrustSettings, ClientError> {
        let body = self
            .exchange(
                "PATCH",
                "/v1/settings/security-trust",
                true,
                None,
                Some(encode_json(request)?),
            )
            .await?;
        decode_direct(&body)
    }
}
