//! Request encoding shared by the Task and Worker operation modules.

use serde::Serialize;

use crate::error::ClientError;

/// Serialize a request DTO once into the exact bytes sent on the wire.
pub fn encode_json<T: Serialize>(value: &T) -> Result<Vec<u8>, ClientError> {
    serde_json::to_vec(value).map_err(|error| ClientError::Decode(error.to_string()))
}
