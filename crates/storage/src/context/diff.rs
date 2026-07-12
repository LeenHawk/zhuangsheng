use serde_json::Value;
use zhuangsheng_core::application::context::ContextDiffEntry;

pub(super) fn diff_values(before: &Value, after: &Value) -> Vec<ContextDiffEntry> {
    let mut changes = Vec::new();
    walk("", Some(before), Some(after), &mut changes);
    changes
}

fn walk(
    path: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    changes: &mut Vec<ContextDiffEntry>,
) {
    if before == after {
        return;
    }
    match (before, after) {
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                walk(
                    &format!("{path}/{}", escape(key)),
                    left.get(key),
                    right.get(key),
                    changes,
                );
            }
        }
        _ => changes.push(ContextDiffEntry {
            path: if path.is_empty() {
                "/".into()
            } else {
                path.into()
            },
            before: before.cloned(),
            after: after.cloned(),
        }),
    }
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::diff_values;

    #[test]
    fn reports_stable_pointer_order_and_array_replacement() {
        let changes = diff_values(
            &json!({"b":1,"a":{"items":[1]}}),
            &json!({"a":{"items":[1,2]},"c":true}),
        );
        assert_eq!(
            changes
                .iter()
                .map(|change| change.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/a/items", "/b", "/c"]
        );
    }
}
