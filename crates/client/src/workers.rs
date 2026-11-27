//! Worker enrollment and fleet administration over any [`Transport`].

use myrlet_protocol::{
    CreateEnrollmentCodeRequest, CreateEnrollmentCodeResponse, RotateWorkerCredentialResponse,
    WorkerDetail, WorkerId, WorkerListResponse,
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
}
