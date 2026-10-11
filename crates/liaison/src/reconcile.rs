//! Keyed diff of desired profile entries against live portal records.
//!
//! A desired entry matches a live record by its id for this portal (from
//! `ids`, written by `admit pull`) or, failing that, by natural key. Matched
//! pairs whose serialized fields differ become updates; unmatched desired
//! entries become creates; unmatched live records are only reported. Nothing
//! is ever deleted.

use admit_domain::PortalIds;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Step<T> {
    Create(T),
    Update { id: i64, fields: Vec<String>, desired: T },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diff<T> {
    pub steps: Vec<Step<T>>,
    /// Natural keys of live records the profile doesn't mention.
    pub extras: Vec<String>,
}

/// Top-level fields that differ, ignoring `ids`.
pub fn changed_fields<T: Serialize>(desired: &T, live: &T) -> Vec<String> {
    let (Ok(Value::Object(mut a)), Ok(Value::Object(mut b))) =
        (serde_json::to_value(desired), serde_json::to_value(live))
    else {
        return vec!["(unserializable)".into()];
    };
    a.remove("ids");
    b.remove("ids");
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
}

pub fn diff<T: Serialize + Clone>(
    portal: &str,
    desired: &[T],
    live: &[(i64, T)],
    key: impl Fn(&T) -> String,
    ids: impl Fn(&T) -> &PortalIds,
) -> Diff<T> {
    let mut taken = vec![false; live.len()];
    let mut steps = Vec::new();
    for d in desired {
        let by_id = ids(d).get(portal).and_then(|want| live.iter().position(|(id, _)| id == want));
        let found = by_id.filter(|&i| !taken[i]).or_else(|| {
            let k = key(d);
            live.iter().enumerate().position(|(i, (_, l))| !taken[i] && key(l) == k)
        });
        match found {
            Some(i) => {
                taken[i] = true;
                let fields = changed_fields(d, &live[i].1);
                if !fields.is_empty() {
                    steps.push(Step::Update { id: live[i].0, fields, desired: d.clone() });
                }
            }
            None => steps.push(Step::Create(d.clone())),
        }
    }
    let extras = live.iter().zip(&taken).filter(|(_, t)| !**t).map(|((_, l), _)| key(l)).collect();
    Diff { steps, extras }
}

#[cfg(test)]
#[path = "tests/reconcile_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/reconcile_serialization_boundaries.rs"]
mod serialization_boundaries;
