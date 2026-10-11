use serde_json::json;

use super::*;

#[test]
fn unmatched_prerequisites_are_counted_per_program() {
    let p = json!([
        {"organizationName": "Yale University", "programName": "GEPN", "optOut": null,
         "prerequisites": [{"courseMatched": false}, {"courseMatched": true}]},
        {"organizationName": "Done", "programName": "All", "prerequisites": [{"courseMatched": true}]},
        {"organizationName": "Skip", "programName": "Out", "optOut": true, "prerequisites": [{"courseMatched": false}]}]);
    assert_eq!(
        check(&p),
        vec![Finding::Note("Yale University - GEPN: 1 of 2 prerequisites matched to courses".into())]
    );
}
