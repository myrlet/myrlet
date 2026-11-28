//! The singleton Security & Trust policies stored in one `hub_settings` row.

use rusqlite::{params, Connection, Transaction};

use myrlet_protocol::{HubEventEnvelope, SecurityTrustSettings, UpdateSecurityTrustRequest};

use crate::error::StoreError;
use crate::events::insert_event;

pub const EVENT_SECURITY_TRUST_UPDATED: &str = "settings.security_trust";

type RawSettings = (i64, i64);

fn map_settings(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSettings> {
    Ok((row.get(0)?, row.get(1)?))
}

fn to_settings(raw: RawSettings) -> SecurityTrustSettings {
    SecurityTrustSettings {
        require_worker_approval: raw.0 != 0,
        require_configuration_apply: raw.1 != 0,
    }
}

/// Read the stored policy pair; the schema always seeds the singleton row.
pub fn load_settings(conn: &Connection) -> Result<SecurityTrustSettings, StoreError> {
    let raw = conn.query_row(
        "SELECT require_worker_approval, require_configuration_apply \
         FROM hub_settings WHERE singleton = 1",
        [],
        map_settings,
    )?;
    Ok(to_settings(raw))
}

fn write_settings(tx: &Transaction<'_>, settings: SecurityTrustSettings) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE hub_settings SET require_worker_approval = ?1, require_configuration_apply = ?2 \
         WHERE singleton = 1",
        params![
            i64::from(settings.require_worker_approval),
            i64::from(settings.require_configuration_apply)
        ],
    )?;
    Ok(())
}

/// Merge a settings patch over the stored pair. Policies never act retroactively.
pub fn update_settings(
    conn: &mut Connection,
    request: &UpdateSecurityTrustRequest,
    now_ms: i64,
) -> Result<(SecurityTrustSettings, HubEventEnvelope), StoreError> {
    let tx = conn.transaction()?;
    let current = load_settings(&tx)?;
    let next = request.clone().merged_over(current);
    write_settings(&tx, next)?;
    let event = insert_event(
        &tx,
        EVENT_SECURITY_TRUST_UPDATED,
        &serde_json::json!({
            "requireWorkerApproval": next.require_worker_approval,
            "requireConfigurationApply": next.require_configuration_apply,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((next, event))
}
