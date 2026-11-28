//! Cross-process resilience: FIFO queue auto-dispatch and restart resync replay, over real
//! `hub`, `worker`, and `myrlet` binaries. Scenarios are grouped by recovery surface so each
//! module reads on its own.

mod all_in_one;
mod dispatch;
mod fixtures;
mod harness;
mod long_task;
