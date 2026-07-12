use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCheckpointView {
    pub id: String,
    pub run_id: String,
    pub context_branch_id: String,
    pub through_seq: u64,
    pub graph_revision_id: String,
    pub head_commit_id: String,
    pub snapshot_ref: String,
    pub effect_watermark: Option<String>,
    pub schema_version: u32,
    pub checksum: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRecoveryView {
    pub run_id: String,
    pub checkpoint_id: String,
    pub replayed_event_count: u64,
    pub recovered_through_seq: u64,
    pub projection_consistent: bool,
}
