use serde_json::{Value, json};

use super::*;
use crate::client::arr;

fn college(name: &str, info: Value, terms: Value) -> LiveCollege {
    LiveCollege { id: 1, name: name.into(), code: String::new(), terms: arr(&terms).to_vec(), info }
}

#[test]
fn the_flag_must_agree_with_in_progress_terms() {
    let live = [
        college(
            "LAVC",
            json!({"stillAttending": false, "collegeEndMonth": "June", "collegeEndYear": "2026"}),
            json!([{"term": "Fall", "year": "2026", "termInProgress": true}]),
        ),
        // A still-attending college reports "Still Attending" as its end month.
        college(
            "LAHC",
            json!({"stillAttending": true, "collegeEndMonth": "Still Attending", "collegeEndYear": ""}),
            json!([
                {"term": "Spring", "year": "2026", "termInProgress": false},
                {"term": "Fall", "year": "2025", "termInProgress": false}]),
        ),
        college(
            "LACC",
            json!({"stillAttending": true}),
            json!([{"term": "Fall", "year": "2026", "termInProgress": true}]),
        ),
    ];
    let f: Vec<String> = check(&live).iter().map(ToString::to_string).collect();
    assert_eq!(
        f,
        [
            "VIOLATION  LAVC: not marked still attending, but Fall 2026 is in progress",
            "PENDING    LAHC: marked still attending, but no term is in progress (last entered: Spring 2026)",
        ]
    );
}
