//! One authenticated Hub WSS session: hello/welcome handshake, resync, lease-bounded
//! heartbeats, and inbound control-frame dispatch. A lost session never proves that
//! local execution stopped.

use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::SinkExt;
use myrlet_protocol::HubHeartbeatAckPayload;
use myrlet_protocol::HubWakeupPayload;
use myrlet_protocol::HubWelcomePayload;
use myrlet_protocol::SlotId;
use myrlet_protocol::TaskCancelPayload;
use myrlet_protocol::TaskCancelledAckPayload;
use myrlet_protocol::WorkerByePayload;
use myrlet_protocol::WorkerHeartbeatPayload;
use myrlet_protocol::WorkerHelloPayload;
use myrlet_protocol::WorkerInventoryAckPayload;
use myrlet_protocol::WorkerResyncPayload;
use myrlet_protocol::WorkerWireEnvelope;
use myrlet_protocol::WorkerWireKind;
use myrlet_protocol::WORKER_PROTOCOL_VERSION;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior};

use super::{RuntimeCore, RuntimeError};
use crate::wss::{self, HubSink, HubSource, WssError};

/// How one WSS session ended.
pub(super) enum Outcome {
    /// Shutdown was requested and `worker.bye` was sent.
    Shutdown,
    /// The connection or lease was lost; reconnect with backoff.
    Lost,
}

/// One event selected from the live session.
enum Step {
    /// The shutdown channel changed; `true` means its sender was dropped.
    Signal(bool),
    Heartbeat,
    LeaseExpired,
    Reconnect,
    /// A locally reloaded inventory must be reported now.
    InventoryChanged,
    Frame(Box<Result<Option<WorkerWireEnvelope<Value>>, WssError>>),
}

impl RuntimeCore {
    /// Serve one connection until shutdown or loss.
    pub(super) async fn session(
        &self,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<Outcome, RuntimeError> {
        let hub_url = self.hub_url();
        let socket = wss::connect(&hub_url, &self.creds.credential, &self.tls).await?;
        let (mut sink, mut source) = wss::split(socket);
        self.send_hello(&mut sink).await?;
        let welcome = await_welcome(&mut source).await?;
        if welcome.worker_id != self.creds.worker_id {
            return Err(RuntimeError::WorkerMismatch {
                reported: welcome.worker_id,
                expected: self.creds.worker_id.clone(),
            });
        }
        if welcome.connection_epoch == 0 {
            return Err(RuntimeError::ZeroEpoch);
        }
        let epoch = welcome.connection_epoch;
        self.epoch.store(epoch, Ordering::SeqCst);
        tracing::info!(
            epoch,
            connection = %welcome.connection_id,
            "worker session established"
        );
        self.send_resync(&mut sink).await?;
        // A reload that happened while disconnected still needs its acknowledgement.
        if self.inventory_pending() {
            self.send_inventory(&mut sink).await?;
        }
        self.retry_pending_claim().await?;
        self.flush_pending(&mut sink).await?;

        let lease_ttl = Duration::from_millis(welcome.lease_ttl_ms.max(1));
        let heartbeat = Duration::from_millis(welcome.heartbeat_interval_ms.max(100));
        let mut ticker = tokio::time::interval(heartbeat);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut deadline = Instant::now() + lease_ttl;

        loop {
            let step = tokio::select! {
                changed = shutdown.changed() => Step::Signal(changed.is_err()),
                () = self.reconnect.notified() => Step::Reconnect,
                () = self.inventory_changed.notified() => Step::InventoryChanged,
                _ = ticker.tick() => {
                    if Instant::now() >= deadline {
                        Step::LeaseExpired
                    } else {
                        Step::Heartbeat
                    }
                }
                frame = wss::next(&mut source) => Step::Frame(Box::new(frame)),
            };
            match step {
                Step::Signal(closed) => {
                    if !closed && !*shutdown.borrow() {
                        continue;
                    }
                    self.shutting_down.store(true, Ordering::SeqCst);
                    self.send_bye(&mut sink).await;
                    let _ = sink.close().await;
                    self.epoch.store(0, Ordering::SeqCst);
                    return Ok(Outcome::Shutdown);
                }
                Step::Reconnect => {
                    // A notification from the preceding session may still be queued.
                    if self.epoch() != epoch {
                        return Ok(self.lost());
                    }
                }
                Step::InventoryChanged => {
                    // Report the reloaded inventory on the live session; claims stay paused
                    // until the Hub acknowledges this revision.
                    if self.inventory_pending() && self.send_inventory(&mut sink).await.is_err() {
                        return Ok(self.lost());
                    }
                }
                Step::LeaseExpired => {
                    tracing::warn!(epoch, "hub lease expired without a frame; reconnecting");
                    return Ok(self.lost());
                }
                Step::Heartbeat => {
                    if let Err(error) = self.send_heartbeat(&mut sink).await {
                        tracing::warn!(%error, "worker.heartbeat could not be sent");
                        return Ok(self.lost());
                    }
                }
                Step::Frame(boxed) => match *boxed {
                    Ok(Some(envelope)) => match self.handle_frame(&mut sink, envelope).await {
                        Ok(true) => deadline = Instant::now() + lease_ttl,
                        Ok(false) => {}
                        Err(error) => {
                            tracing::warn!(%error, "inbound frame could not be handled");
                            return Ok(self.lost());
                        }
                    },
                    Ok(None) => {
                        tracing::info!(epoch, "hub closed the worker session");
                        return Ok(self.lost());
                    }
                    Err(error) => {
                        tracing::warn!(%error, "worker session read failed");
                        return Ok(self.lost());
                    }
                },
            }
        }
    }

    /// Forget the live epoch; pending HTTP reports then wait for the next `hub.welcome`.
    fn lost(&self) -> Outcome {
        self.epoch.store(0, Ordering::SeqCst);
        Outcome::Lost
    }

    async fn handle_frame(
        &self,
        sink: &mut HubSink,
        envelope: WorkerWireEnvelope<Value>,
    ) -> Result<bool, RuntimeError> {
        if envelope.worker_id != self.creds.worker_id {
            return Ok(false);
        }
        let current = self.epoch.load(Ordering::SeqCst);
        if envelope
            .connection_epoch
            .is_some_and(|epoch| epoch != current)
        {
            return Ok(false); // late frame from a fenced connection
        }
        let WorkerWireEnvelope { kind, payload, .. } = envelope;
        match kind {
            WorkerWireKind::HubHeartbeatAck => {
                let _: HubHeartbeatAckPayload = serde_json::from_value(payload)?;
            }
            WorkerWireKind::WorkerInventoryAck => {
                let ack: WorkerInventoryAckPayload = serde_json::from_value(payload)?;
                if ack.accepted {
                    self.inventory_acknowledged(ack.revision);
                }
            }
            WorkerWireKind::TaskCancelledAck => {
                let ack: TaskCancelledAckPayload = serde_json::from_value(payload)?;
                self.mark_cancel_acked(&ack.delivery_id, ack.recorded);
            }
            WorkerWireKind::HubWakeup => {
                let wakeup: HubWakeupPayload = serde_json::from_value(payload)?;
                self.handle_wakeup(wakeup).await?;
            }
            WorkerWireKind::TaskCancel => {
                let cancel: TaskCancelPayload = serde_json::from_value(payload)?;
                self.handle_cancel(sink, cancel).await?;
            }
            _ => {}
        }
        Ok(true)
    }

    async fn send_hello(&self, sink: &mut HubSink) -> Result<(), RuntimeError> {
        let payload = WorkerHelloPayload {
            inventory: self.inventory(),
            active_delivery_ids: self.active_delivery_ids(),
        };
        let envelope = self.envelope(WorkerWireKind::WorkerHello, None, None, payload);
        wss::send(sink, &envelope).await?;
        Ok(())
    }

    async fn send_inventory(&self, sink: &mut HubSink) -> Result<(), RuntimeError> {
        self.send_frame(
            sink,
            WorkerWireKind::WorkerInventory,
            None,
            self.inventory(),
        )
        .await
    }

    async fn send_resync(&self, sink: &mut HubSink) -> Result<(), RuntimeError> {
        let payload = WorkerResyncPayload {
            active_delivery_ids: self.active_delivery_ids(),
        };
        self.send_frame(sink, WorkerWireKind::WorkerResync, None, payload)
            .await
    }

    async fn send_heartbeat(&self, sink: &mut HubSink) -> Result<(), RuntimeError> {
        let payload = WorkerHeartbeatPayload {
            active_delivery_ids: self.active_delivery_ids(),
        };
        self.send_frame(sink, WorkerWireKind::WorkerHeartbeat, None, payload)
            .await
    }

    async fn send_bye(&self, sink: &mut HubSink) {
        let payload = WorkerByePayload {
            reason: Some("worker shutting down".to_owned()),
        };
        if let Err(error) = self
            .send_frame(sink, WorkerWireKind::WorkerBye, None, payload)
            .await
        {
            tracing::warn!(%error, "worker.bye could not be sent");
        }
    }

    /// Send one envelope stamped with the current epoch (when one is live).
    pub(super) async fn send_frame<T: Serialize>(
        &self,
        sink: &mut HubSink,
        kind: WorkerWireKind,
        slot_id: Option<SlotId>,
        payload: T,
    ) -> Result<(), RuntimeError> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        let envelope = self.envelope(kind, slot_id, (epoch > 0).then_some(epoch), payload);
        wss::send(sink, &envelope).await?;
        Ok(())
    }

    fn envelope<T>(
        &self,
        kind: WorkerWireKind,
        slot_id: Option<SlotId>,
        connection_epoch: Option<u64>,
        payload: T,
    ) -> WorkerWireEnvelope<T> {
        WorkerWireEnvelope {
            v: WORKER_PROTOCOL_VERSION.to_owned(),
            kind,
            message_id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            worker_id: self.creds.worker_id.clone(),
            slot_id,
            connection_id: None,
            connection_epoch,
            sent_at_ms: crate::now_ms(),
            reply_to: None,
            payload,
        }
    }
}

async fn await_welcome(source: &mut HubSource) -> Result<HubWelcomePayload, RuntimeError> {
    loop {
        let Some(envelope) = wss::next(source).await? else {
            return Err(RuntimeError::NoWelcome);
        };
        if envelope.kind == WorkerWireKind::HubWelcome {
            return Ok(serde_json::from_value(envelope.payload)?);
        }
    }
}
