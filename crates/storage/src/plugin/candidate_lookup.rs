use sea_orm::ConnectionTrait;
use zhuangsheng_core::application::plugin::PluginCandidateView;

use crate::{StorageResult, graph::helpers::sql};

use super::rows::candidate_from_row;

pub(super) async fn load_staged_candidate_by_commit<C: ConnectionTrait>(
    db: &C,
    plugin_id: &str,
    resolved_commit: &str,
) -> StorageResult<Option<PluginCandidateView>> {
    db.query_one_raw(sql(
        "SELECT * FROM plugin_candidates WHERE plugin_id = ? AND resolved_commit = ? AND status = 'staged' ORDER BY created_at DESC LIMIT 1",
        vec![plugin_id.into(), resolved_commit.into()],
    ))
    .await?
    .as_ref()
    .map(candidate_from_row)
    .transpose()
}
