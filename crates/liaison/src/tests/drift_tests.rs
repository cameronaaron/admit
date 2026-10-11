use serde_json::json;

use super::*;

fn programs(p: Value) -> Shape {
    shape(&json!({"programs": [p], "timeZone": "EST"}))
}

#[test]
fn a_shape_is_every_key_path_with_its_type_and_no_values() {
    let s = shape(&json!({"programs": [{"deadline": "2027-01-16", "id": 1}, {"deadline": null, "id": 2, "x": true}],
        "collegeMap": {"74212": {"orderedTRF": false}}, "empty": []}));
    let want: Shape = [
        ("$", "object"),
        ("$.programs", "array"),
        ("$.programs[]", "object"),
        ("$.programs[].deadline", "string"),
        ("$.programs[].id", "number"),
        ("$.programs[].x", "bool"),
        ("$.collegeMap", "object"),
        ("$.collegeMap.*", "object"),
        ("$.collegeMap.*.orderedTRF", "bool"),
        ("$.empty", "array"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(s, want);
    assert!(!format!("{s:?}").contains("2027"), "no values are kept");
    assert_eq!(shape(&json!([1, "a"]))["$[]"], "mixed");
    // Live 2026-10-07: /transcripts keys orgMap by school name.
    let by_name = shape(&json!({"orgMap": {"Yale University": [{"organizationName": "Yale"}], "_x1": 1, "": 2}}));
    assert_eq!(
        by_name.keys().map(String::as_str).collect::<Vec<_>>(),
        ["$", "$.orgMap", "$.orgMap.*", "$.orgMap.*[]", "$.orgMap.*[].organizationName", "$.orgMap._x1"]
    );
    let mut m = shape(&json!({"a": null}));
    merge(&mut m, &shape(&json!({"a": 1})));
    assert_eq!(m["$.a"], "number", "a null widens to the type seen elsewhere");
}

#[test]
fn a_removed_or_retyped_field_is_a_violation_and_an_added_one_a_note() {
    let rec = programs(json!({"deadline": "2027-01-16", "deadlineDisplay": "Jan 15, 2027", "programSelectedId": 1}));
    assert!(compare("S", &rec, &rec).is_empty());

    let renamed = programs(json!({"deadlineDate": "2027-01-16", "deadlineDisplay": "Jan 15", "programSelectedId": 1}));
    let f = compare("S", &rec, &renamed);
    assert_eq!(
        f,
        vec![
            Finding::Violation("S: $.programs[].deadline (string) is gone".into()),
            Finding::Note("S: 1 field(s) not in the recording (admit drift --record accepts them)".into()),
        ]
    );

    let retyped = programs(json!({"deadline": 1798000000000_i64, "deadlineDisplay": "x", "programSelectedId": 1}));
    assert_eq!(
        compare("S", &rec, &retyped),
        vec![Finding::Violation("S: $.programs[].deadline was string, is now number".into())]
    );
}

#[test]
fn absence_under_an_empty_list_or_null_parent_proves_nothing() {
    let rec = programs(json!({"deadline": "d", "fee": {"amount": 1}}));
    assert!(compare("S", &rec, &shape(&json!({"programs": [], "timeZone": "EST"}))).is_empty(), "empty list");
    assert!(compare("S", &rec, &programs(json!({"deadline": "d", "fee": null}))).is_empty(), "null parent");
    assert!(compare("S", &rec, &programs(json!({"deadline": null, "fee": {"amount": null}}))).is_empty(), "nulls");
}

#[test]
fn a_missing_list_is_empty_unless_a_new_key_took_its_place() {
    // Captured 2026-10-06 before any achievement existed: {"optOut": false}.
    let rec = shape(&json!({"achievements": [{"id": 1}], "optOut": false}));
    let f = compare("A", &rec, &shape(&json!({"optOut": false})));
    assert_eq!(f, vec![Finding::Note("A: $.achievements absent (an empty list is left out)".into())]);
    let f = compare("A", &rec, &shape(&json!({"items": [{"id": 1}], "optOut": false})));
    assert_eq!(f[0], Finding::Violation("A: $.achievements (array) is gone".into()), "renamed: {f:?}");
    let f = compare("A", &rec, &shape(&json!({"achievements": [{"ident": 1}], "optOut": false})));
    assert_eq!(f[0], Finding::Violation("A: $.achievements[].id (number) is gone".into()), "{f:?}");
    let rec = shape(&json!({"a": {"x": 1}, "b": 1}));
    assert_eq!(compare("O", &rec, &shape(&json!({"b": 1})))[0], Finding::Violation("O: $.a (object) is gone".into()));
}

#[test]
fn an_endpoint_missing_from_either_side_is_a_violation() {
    let one = |e: &str| Shapes::from([(e.to_string(), shape(&json!({"a": 1})))]);
    assert!(check(&one("/x"), &one("/x")).is_empty());
    let mut both = one("/x");
    both.extend(one("/y"));
    assert_eq!(check(&both, &one("/x")), vec![Finding::Violation("/y: recorded but no longer read".into())]);
    assert_eq!(
        check(&one("/x"), &both),
        vec![Finding::Violation("/y: read but never recorded (admit drift --record)".into())]
    );
}
