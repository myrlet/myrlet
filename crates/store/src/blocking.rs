//! One-permit bridge from async Hub handlers to the single blocking SQLite writer.

use std::sync::Mutex;

use rusqlite::Connection;
use tokio::runtime::{Handle, RuntimeFlavor};
use tokio::sync::Semaphore;

use crate::error::StoreError;

/// Single-writer SQLite handle: one permit, one connection, never parked on an async worker.
pub struct BlockingSqlite {
    connection: Mutex<Connection>,
    writer: Semaphore,
}

impl BlockingSqlite {
    pub fn new(connection: Connection) -> Self {
        Self {
            connection: Mutex::new(connection),
            writer: Semaphore::new(1),
        }
    }

    /// Run `f` with exclusive access to the connection.
    ///
    /// A multi-thread Tokio runtime moves other tasks to sibling workers and blocks this
    /// worker via `block_in_place`; a current-thread runtime has no sibling worker to
    /// starve, so the closure simply runs inline.
    pub async fn call<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let _permit = self
            .writer
            .acquire()
            .await
            .map_err(|_| StoreError::invalid("store writer is closed"))?;
        let work = || match self.connection.lock() {
            Ok(mut guard) => f(&mut guard),
            Err(_) => Err(StoreError::invalid("store connection lock is poisoned")),
        };
        match Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(work)
            }
            _ => work(),
        }
    }
}
