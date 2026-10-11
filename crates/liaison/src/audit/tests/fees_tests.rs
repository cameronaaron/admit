use serde_json::json;

use super::*;

#[test]
fn unpaid_unsubmitted_programs_are_listed_once() {
    let sel = json!([
        {"programName": "EL MSN", "paymentStatus": false, "submissionStatus": "In Progress", "feeWaiverStatus": "Requested"},
        {"programName": "ABSN", "paymentStatus": true, "submissionStatus": "In Progress", "feeWaiverStatus": "Requested"},
        {"programName": "Old", "paymentStatus": false, "submissionStatus": "Submitted", "feeWaiverStatus": null}]);
    assert_eq!(check(&sel), vec![Finding::Pending("1 program(s) not paid (fee waiver Requested): EL MSN".into())]);
    assert!(check(&json!([])).is_empty());
}
