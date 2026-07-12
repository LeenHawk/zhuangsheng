use std::collections::{HashMap, HashSet, VecDeque};

use sea_orm::ConnectionTrait;

use crate::{StorageError, StorageResult, graph::helpers::sql};

const MAX_COMMITS: usize = 10_000;
const MAX_BASE_WORK: usize = 1_000_000;

pub(super) struct MergeHeads {
    pub source: String,
    pub target: String,
}

pub(super) async fn load_heads<C: ConnectionTrait>(
    connection: &C,
    context_id: &str,
    source_branch_id: &str,
    target_branch_id: &str,
) -> StorageResult<MergeHeads> {
    if source_branch_id == target_branch_id {
        return Err(StorageError::InvalidArgument(
            "merge branches must be distinct".into(),
        ));
    }
    let rows = connection.query_all_raw(sql(
        "SELECT id, head_commit_id, status FROM context_branches WHERE context_id = ? AND id IN (?, ?)",
        vec![context_id.into(), source_branch_id.into(), target_branch_id.into()],
    )).await?;
    if rows.len() != 2 {
        return Err(StorageError::NotFound {
            kind: "context_branch",
            id: format!("{source_branch_id}|{target_branch_id}"),
        });
    }
    let mut source = None;
    let mut target = None;
    for row in rows {
        if row.try_get::<String>("", "status")? != "active" {
            return Err(StorageError::Conflict("merge_branch_not_active"));
        }
        let id: String = row.try_get("", "id")?;
        let head: String = row.try_get("", "head_commit_id")?;
        if id == source_branch_id {
            source = Some(head);
        } else if id == target_branch_id {
            target = Some(head);
        }
    }
    Ok(MergeHeads {
        source: source.ok_or_else(|| StorageError::Integrity("source branch missing".into()))?,
        target: target.ok_or_else(|| StorageError::Integrity("target branch missing".into()))?,
    })
}

pub(super) async fn unique_merge_base<C: ConnectionTrait>(
    connection: &C,
    source_head: &str,
    target_head: &str,
) -> StorageResult<String> {
    let source = ancestors(connection, source_head).await?;
    let target = ancestors(connection, target_head).await?;
    let common: HashSet<String> = source
        .keys()
        .filter(|id| target.contains_key(*id))
        .cloned()
        .collect();
    if common.is_empty() {
        return Err(StorageError::Conflict("merge_base_missing"));
    }
    let mut non_maximal = HashSet::new();
    let mut work = 0;
    for candidate in &common {
        let mut queue: VecDeque<String> = source
            .get(candidate)
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let mut seen = HashSet::new();
        while let Some(ancestor) = queue.pop_front() {
            work += 1;
            if work > MAX_BASE_WORK {
                return Err(StorageError::Integrity(
                    "merge base analysis limit exceeded".into(),
                ));
            }
            if !seen.insert(ancestor.clone()) {
                continue;
            }
            if common.contains(&ancestor) {
                non_maximal.insert(ancestor.clone());
            }
            if let Some(parents) = source.get(&ancestor) {
                queue.extend(parents.iter().cloned());
            }
        }
    }
    let maximal: Vec<_> = common.difference(&non_maximal).cloned().collect();
    match maximal.as_slice() {
        [base] => Ok(base.clone()),
        _ => Err(StorageError::Conflict("ambiguous_merge_base")),
    }
}

async fn ancestors<C: ConnectionTrait>(
    connection: &C,
    head: &str,
) -> StorageResult<HashMap<String, Vec<String>>> {
    let mut graph = HashMap::new();
    let mut queue = VecDeque::from([head.to_owned()]);
    while let Some(commit) = queue.pop_front() {
        if graph.contains_key(&commit) {
            continue;
        }
        if graph.len() >= MAX_COMMITS {
            return Err(StorageError::Integrity(
                "merge ancestry exceeds traversal limit".into(),
            ));
        }
        let rows = connection.query_all_raw(sql(
            "SELECT parent_commit_id FROM commit_parents WHERE commit_id = ? ORDER BY parent_order",
            vec![commit.clone().into()],
        )).await?;
        let parents: Vec<String> = rows
            .iter()
            .map(|row| row.try_get("", "parent_commit_id"))
            .collect::<Result<_, _>>()?;
        queue.extend(parents.iter().cloned());
        graph.insert(commit, parents);
    }
    Ok(graph)
}
