use super::*;
use admit_domain::Recommender;

const YALE: &str = "Yale University - GEPN";

/// The captured evaluation section (2026-10-07), one "Letter Upload Only" type.
fn doc() -> Value {
    json!({"sectionId": 2013254, "evaluationTypes": [{"minRequired": 2, "maxAllowed": 4,
        "type": "Letter Upload Only", "evaluationTypeConstant": "Letter Upload Only",
        "evaluationTypeDisplayName": null}],
        "allEvaluationRequestList": {"evaluations": []}, "data": {"evaluations": []},
        "evalSubmitDeadline": "2028-02-10"})
}

fn program(doc: Value) -> LiveProgram {
    LiveProgram { id: 79439764, label: YALE.into(), doc }
}

fn jane() -> Recommender {
    Recommender {
        first_name: " Jane ".into(),
        last_name: "Doe".into(),
        email: "Jane@School.example ".into(),
        due: "2026-10-25".into(),
        waive: true,
        message: " Thank you. ".into(),
        programs: vec![YALE.into()],
        kind: None,
    }
}

fn file(rs: Vec<Recommender>) -> ReferencesFile {
    ReferencesFile { consent: true, recommenders: rs }
}

#[test]
fn a_new_recommender_is_one_request_with_the_forms_body() {
    let (qs, f) = plan(&file(vec![jane()]), &[program(doc())]);
    assert_eq!(f, vec![]);
    let [q] = qs.as_slice() else { panic!("{qs:?}") };
    assert_eq!(q.program_id, 79439764);
    assert_eq!(
        body(q),
        json!({"committeeEvaluation": false, "evaluationType": "Letter Upload Only",
            "evaluationTypeDisplayName": "Letter Upload Only",
            "permissionForSchoolsToContactReference": true, "permissionToContactReference": true,
            "waiverOfEvaluation": true,
            "evaluator": {"dueDate": "2026-10-25", "emailAddress": "Jane@School.example", "evalFirstName": "Jane",
                "evalLastName": "Doe", "message": "Thank you.", "strDueDate": "2026-10-25"},
            "reSendEvaluatorEmail": false})
    );
    assert_eq!(
        verify_path(q),
        "/emailVerification/pooledEvaluations/email/Jane@School.example/evaluationType/Letter%20Upload%20Only"
    );
}

#[test]
fn an_address_already_asked_is_never_asked_again() {
    for list in ["data", "allEvaluationRequestList"] {
        let mut d = doc();
        d[list]["evaluations"] = json!([{"id": 1, "evaluator": {"emailAddress": "jane@school.example"}}]);
        assert_eq!(plan(&file(vec![jane()]), &[program(d)]), (vec![], vec![]), "{list}");
    }
}

#[test]
fn nothing_is_planned_without_consent() {
    let mut f = file(vec![jane()]);
    f.consent = false;
    assert_eq!(plan(&f, &[program(doc())]), (vec![], vec![]));
}

#[test]
fn the_program_type_deadline_and_maximum_are_checked() {
    let violation = |rs: Vec<Recommender>, d: Value| {
        let (qs, f) = plan(&file(rs), &[program(d)]);
        f.iter()
            .map(ToString::to_string)
            .chain(qs.iter().map(|q| format!("planned {}", q.recommender.email.trim())))
            .collect::<Vec<_>>()
    };
    let mut r = jane();
    r.programs = vec!["Nowhere - Nothing".into()];
    assert_eq!(
        violation(vec![r], doc()),
        [
            "VIOLATION  references.toml: \"Nowhere - Nothing\" (Jane@School.example) is not a program asking for evaluations"
        ]
    );

    let mut two = doc();
    two["evaluationTypes"].as_array_mut().expect("types").push(json!({"type": "Committee Letters",
        "evaluationTypeConstant": "COMMITTEE", "evaluationTypeDisplayName": "Committee"}));
    assert_eq!(
        violation(vec![jane()], two.clone()),
        [format!(
            "VIOLATION  {YALE}: Jane@School.example: type = one of \"Letter Upload Only\", \"Committee Letters\""
        )]
    );
    let mut r = jane();
    r.kind = Some("committee letters".into());
    let (qs, _) = plan(&file(vec![r]), &[program(two)]);
    let b = body(&qs[0]);
    assert_eq!(
        (b["committeeEvaluation"].clone(), b["evaluationTypeDisplayName"].clone()),
        (json!(true), json!("Committee"))
    );
    let mut none = doc();
    none["evaluationTypes"] = json!([]);
    assert_eq!(
        violation(vec![jane()], none),
        [format!("VIOLATION  {YALE}: Jane@School.example: type = one of (none offered)")]
    );

    let mut late = jane();
    late.due = "2028-02-11".into();
    assert_eq!(
        violation(vec![late], doc()),
        [format!("VIOLATION  {YALE}: Jane@School.example: due 2028-02-11 is after the evaluation deadline 2028-02-10")]
    );
    let mut open = doc();
    open["evalSubmitDeadline"] = Value::Null;
    let mut far = jane();
    far.due = "2099-01-01".into();
    assert_eq!(violation(vec![far], open), ["planned Jane@School.example"], "no deadline given: none checked");

    // Three already asked, max 4: the first new one fits, the second not.
    let mut full = doc();
    full["data"]["evaluations"] = json!([{"evaluator": {"emailAddress": "a@x-edu.example"}}, {"evaluator": {"emailAddress": "b@x-edu.example"}}, {"evaluator": {"emailAddress": ""}}, {"evaluator": {"emailAddress": "c@x-edu.example"}}]);
    let mut other = jane();
    other.email = "z@x-edu.example".into();
    assert_eq!(
        violation(vec![jane(), other], full),
        [
            format!("VIOLATION  {YALE}: z@x-edu.example: the program takes at most 4 evaluation(s)"),
            "planned Jane@School.example".into()
        ]
    );
}
