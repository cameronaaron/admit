use serde_json::json;

use super::*;

#[test]
fn counts_against_the_minimum() {
    // Captured shape (UCSB ABSN, 2026-10-06).
    let ev = json!({"optOut": false, "evaluationTypes": [{"minRequired": 2, "maxAllowed": 4}],
        "evaluationsCompleted": 0, "evaluationsRequested": 0, "evalReqLastDate": "2028-01-28 11:59:00"});
    assert_eq!(
        check("UCSB - ABSN", &ev),
        vec![Finding::Pending("UCSB - ABSN: 0 of 2 recommendation(s) requested".into())]
    );
    let ev = json!({"evaluationTypes": [{"minRequired": 2}], "evaluationsCompleted": 1, "evaluationsRequested": 2});
    assert_eq!(check("X", &ev), vec![Finding::Pending("X: 1 of 2 recommendation(s) received".into())]);
    let ev = json!({"evaluationTypes": [{"minRequired": 2}], "evaluationsCompleted": 2, "evaluationsRequested": 3});
    assert!(check("X", &ev).is_empty());
    assert!(check("X", &json!({"optOut": true, "evaluationTypes": [{"minRequired": 2}]})).is_empty());
}

#[test]
fn no_minimum_means_no_recommendation_requirement() {
    assert!(check("test", &json!({"evaluationTypes":[]})).is_empty());
}

#[test]
fn a_row_carries_counts_and_the_deadline_and_none_when_nothing_is_required() {
    let d = time::macros::date!(2026 - 11 - 01);
    let ev = json!({"optOut": false, "evaluationTypes": [{"minRequired": 1}, {"minRequired": 3}],
        "evaluationsRequested": 2, "evaluationsCompleted": 1});
    assert_eq!(
        read("Yale - GEPN", &ev, Some(d)),
        Some(Recs { label: "Yale - GEPN".into(), need: 3, requested: 2, received: 1, deadline: Some(d) })
    );
    assert_eq!(read("X", &json!({"optOut": true, "evaluationTypes": [{"minRequired": 2}]}), Some(d)), None);
    assert_eq!(read("X", &json!({"evaluationTypes": []}), None), None);
}
