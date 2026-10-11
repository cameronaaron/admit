use serde_json::json;

use super::*;

#[test]
fn required_incomplete_is_a_violation_optional_is_pending() {
    let nav = json!([{"sections": [
        {"title": "Transcript Entry", "name": "transcripts", "completed": 0, "sectionRequired": true},
        {"title": "Experiences", "name": "experiences", "completed": 0, "sectionRequired": false},
        {"title": "GDPR", "name": "gdpr", "completed": 1, "sectionRequired": true}]}]);
    let f: Vec<String> = from_navigation(&nav).iter().map(ToString::to_string).collect();
    assert_eq!(f.len(), 2);
    assert!(f[0].starts_with("VIOLATION") && f[0].contains("Transcript Entry"));
    assert!(f[1].starts_with("PENDING") && f[1].contains("Experiences"));
}

#[test]
fn program_sections_list_what_is_left() {
    let p = json!([{"orgName": "UC Davis", "programName": "MEPN", "status": "INCOMPLETE", "subSections": [
        {"title": null, "name": "Questions", "completed": false},
        {"title": "References", "name": "Evaluation Types", "completed": false},
        {"title": "Documents", "name": "Documents", "completed": true}]}]);
    let f = from_programs(&p);
    assert_eq!(f, vec![Finding::Pending("UC Davis - MEPN [INCOMPLETE]: Questions, References".into())]);
}

#[test]
fn a_program_with_nothing_left_is_not_listed() {
    let p = json!([{"orgName": "UC Davis", "programName": "MEPN", "status": "COMPLETE", "subSections": [
        {"title": "Documents", "name": "Documents", "completed": true}]}]);
    assert!(from_programs(&p).is_empty());
}
