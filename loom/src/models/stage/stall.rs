//! The record a stage keeps once loom stops recovering it from stalls.

use serde::{Deserialize, Serialize};

/// The stall that found a stage's automatic stall recoveries already spent.
///
/// Written by `event_handler::recover_hung` when it leaves the stage for an
/// operator. `loom status` reports the stage as stalled only while
/// `session_id` is still the stage's session and has stayed silent at least
/// `silent_secs`, so a reset, a successor session, or a heartbeat retires the
/// record without a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StallExhaustion {
    /// The session that went silent.
    pub session_id: String,
    /// How long it had been silent when loom stopped recovering the stage.
    pub silent_secs: u64,
}
