use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSelectionView {
    pub turn_id: String,
    pub selected_run_id: String,
    pub selected_branch_id: String,
    pub selected_commit_id: String,
    pub selected_at: i64,
}
