use serde_json::json;

use super::*;

#[test]
fn a_body_without_a_college_map_checks_as_nothing_missing() {
    assert!(check(&json!({}), &[]).is_empty());
    assert!(check(&json!({"collegeMap": "not a map"}), &[]).is_empty());
}

#[test]
fn missing_official_transcripts_say_whether_they_were_ordered() {
    let c = json!({"collegeMap": {
        "1": {"collegeName": "CONNECTICUT COLLEGE", "orderedTRF": "2026-07-23",
              "officialTranscript": {"transcriptReceived": true}, "domesticTranscriptsNotRequired": false},
        "2": {"collegeName": "LOS ANGELES VALLEY COLLEGE", "orderedTRF": null,
              "officialTranscript": {"transcriptReceived": false}, "domesticTranscriptsNotRequired": false},
        "3": {"collegeName": "ELMBRIDGE UNIVERSITY", "orderedTRF": "2026-07-26",
              "officialTranscript": {"transcriptReceived": false}, "domesticTranscriptsNotRequired": false},
        "4": {"collegeName": "EXEMPT", "orderedTRF": null,
              "officialTranscript": {"transcriptReceived": false}, "domesticTranscriptsNotRequired": true}}});
    let f: Vec<String> = check(&c, &[]).iter().map(ToString::to_string).collect();
    assert_eq!(
        f,
        [
            "PENDING    official transcript from ELMBRIDGE UNIVERSITY ordered 2026-07-26, not yet received",
            "PENDING    official transcript from LOS ANGELES VALLEY COLLEGE not ordered",
        ]
    );
}

#[test]
fn a_shared_transcript_received_for_one_college_covers_the_group() {
    let c = json!({"collegeMap": {
        "1": {"collegeName": "LOS ANGELES CITY COLLEGE", "orderedTRF": "2026-07-23",
              "officialTranscript": {"transcriptReceived": true}},
        "2": {"collegeName": "LOS ANGELES VALLEY COLLEGE", "orderedTRF": null,
              "officialTranscript": {"transcriptReceived": false}},
        "3": {"collegeName": "ELMBRIDGE UNIVERSITY", "orderedTRF": null,
              "officialTranscript": {"transcriptReceived": false}}}});
    let laccd = vec![vec!["LOS ANGELES CITY COLLEGE".to_string(), "LOS ANGELES VALLEY COLLEGE".to_string()]];
    let f = check(&c, &laccd);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(
        f.iter().any(|x| matches!(x, Finding::Note(m) if m.starts_with(
            "official transcript from LOS ANGELES VALLEY COLLEGE: on the shared transcript received for LOS ANGELES CITY COLLEGE"
        ))),
        "{f:?}"
    );
    assert!(
        f.contains(&Finding::Pending("official transcript from ELMBRIDGE UNIVERSITY not ordered".into())),
        "outside the group: {f:?}"
    );
}
