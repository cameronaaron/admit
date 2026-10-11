use super::*;

const YALE: &str = "Yale University - GEPN";

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/yale_questions.json")).expect("fixture")
}

fn program(doc: Value) -> LiveProgram {
    LiveProgram { id: 79439764, label: YALE.into(), doc }
}

fn answers(pairs: &[(&str, &str)]) -> Vec<ProgramAnswers> {
    vec![ProgramAnswers {
        name: YALE.into(),
        portal: "nursingcas".into(),
        answers: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }]
}

#[test]
fn the_pulled_answers_planned_on_the_blank_form_are_the_save_the_browser_sent() {
    let fx = fixture();
    let pulled = pull(&[program(fx["after"].clone())], "nursingcas");
    assert_eq!(pulled[0].answers.len(), 7, "{:?}", pulled[0].answers);
    assert_eq!(pulled[0].answers["3113119"], "No");
    let (saves, f) = plan(&pulled, "nursingcas", &[program(fx["before"].clone())]);
    assert_eq!(
        f,
        vec![Finding::Pending(format!(
            "{YALE}: 1 required question(s) unanswered: 3113132 (Personal Statement (3,000 characters))"
        ))]
    );
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].changes.len(), 7);
    let b = body(&saves[0].doc, &saves[0].changes);
    assert_eq!(b, fx["save"]["request"], "byte for byte");
    assert_eq!(impact_body(&b), fx["impact"]["request"]);
    assert_eq!(impacts(&fx["impact"]["response"]), None, "the captured check reported nothing");
    // After the save, nothing is left to write.
    let (again, _) = plan(&pulled, "nursingcas", &[program(fx["after"].clone())]);
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn answers_that_name_nothing_or_overrun_the_limit_are_violations() {
    let fx = fixture();
    let long = "x".repeat(3501);
    let mut a = answers(&[("999", "Yes"), ("3113119", "Maybe"), ("3113132", &long), ("3113125", "  yes ")]);
    a.push(ProgramAnswers { name: "Nowhere - Nothing".into(), portal: "nursingcas".into(), answers: BTreeMap::new() });
    a.push(ProgramAnswers { name: YALE.into(), portal: "othercas".into(), answers: BTreeMap::new() });
    let (saves, f) = plan(&a, "nursingcas", &[program(fx["before"].clone())]);
    let text: Vec<String> = f.iter().map(ToString::to_string).collect();
    assert_eq!(
        text[0],
        "VIOLATION  questions.toml: \"Nowhere - Nothing\" is not a program with questions on nursingcas"
    );
    assert!(text.contains(&format!("VIOLATION  {YALE}: no question 999 on the portal")), "{text:#?}");
    assert!(
        text.contains(&format!("VIOLATION  {YALE}: question 3113119: \"Maybe\" is not a choice (\"No\", \"Yes\")")),
        "{text:#?}"
    );
    assert!(
        text.contains(&format!(
            "VIOLATION  {YALE}: question 3113132 (Personal Statement (3,000 characters)): 3501 characters, the limit is 3500"
        )),
        "{text:#?}"
    );
    assert_eq!(saves[0].changes.len(), 1, "\"  yes \" matches \"Yes\" ignoring case and spaces");
    assert_eq!(saves[0].changes[0].to, "Yes");
    assert!(!saves[0].complete, "required questions are still blank");

    // A kind no captured save holds is not written.
    let doc =
        json!({"questionGroups": [{"questions": [{"id": 5, "question": "Pick", "input": {"type": "checkbox"}}]}]});
    let (saves, f) = plan(&answers(&[("5", "A")]), "nursingcas", &[program(doc)]);
    assert!(saves.is_empty());
    assert_eq!(
        f,
        vec![Finding::Violation(format!(
            "{YALE}: question 5 (Pick) is a checkbox answer, which admit does not write (none was captured); enter it in the browser"
        ))]
    );
}

const DAVIS: &str = "University of California - Davis - MEPN";

#[test]
fn typed_answers_are_written_and_a_finished_form_is_saved_complete() {
    // UC Davis, 2026-10-07: every required question answered in one save,
    // three of them typed, then the section marked complete.
    let fx: Value = serde_json::from_str(include_str!("fixtures/davis_questions.json")).expect("fixture");
    let davis = |doc: &Value| LiveProgram { id: 79437728, label: DAVIS.into(), doc: doc.clone() };
    let pulled = pull(&[davis(&fx["after"])], "nursingcas");
    assert_eq!(pulled[0].answers["3110250"], "test", "the essay holds the capture's placeholder");
    let (saves, f) = plan(&pulled, "nursingcas", &[davis(&fx["before"])]);
    // The 5000-character essay holding "test" is flagged; "LACCD" (300) and
    // "N/A" (1000) are short answers in short boxes, and are not.
    let placeholder = vec![Finding::Pending(format!(
        "{DAVIS}: question 3110250 (In the space provided, write a brief statement expressing yo) is answered with 4 character(s) (\"test\") of 5000: a placeholder?"
    ))];
    assert_eq!(f, placeholder);
    assert!(saves[0].complete, "nothing required is left blank");
    let b = body(&saves[0].doc, &saves[0].changes);
    assert_eq!(b, fx["save"]["request"], "byte for byte, textareas included");
    assert_eq!(impact_body(&b), fx["impact"]["request"], "typed answers are not part of the check");
    let (saves, f) = plan(&pulled, "nursingcas", &[davis(&fx["after"])]);
    assert!(saves.is_empty(), "converged");
    assert_eq!(f, placeholder, "still flagged once it is on the portal");
    let mut essay = pulled.clone();
    essay[0].answers.insert("3110250".into(), "x".repeat(100));
    assert!(plan(&essay, "nursingcas", &[davis(&fx["after"])]).1.is_empty(), "100 characters is an answer");
}

#[test]
fn hidden_questions_wait_for_the_choice_that_shows_them() {
    // 3113121 shows only when 3113119 ("Joint Degree Program?") is 35208944.
    let fx = fixture();
    let q = |doc: &Value, id: i64| questions(doc).find(|q| q["id"] == id).expect("question").clone();
    let joint = q(&fx["before"], 3113121);
    assert_eq!(joint["visibilityRules"], json!([{"operator": "or", "rules": ["35208944"]}]));
    assert!(!visible(&joint, &[]) && visible(&joint, &["35208944".into()]));
    let both = json!({"visibilityRules": [{"operator": "and", "rules": ["1", "2"]}]});
    assert!(!visible(&both, &["1".into()]) && visible(&both, &["1".into(), "2".into()]));
    let (_, f) = plan(&answers(&[("3113119", "Yes")]), "nursingcas", &[program(fx["before"].clone())]);
    assert!(f[0].message().contains("3113121 (Students must"), "answering Yes shows the follow-up: {f:?}");
}

#[test]
fn an_impact_on_other_programs_is_reported_and_text_strips_markup() {
    let fx = fixture();
    let mut r = fx["impact"]["response"].clone();
    r["extendedProfileResponse"]["conflictedPrograms"] = json!([{"programId": 7}]);
    assert_eq!(impacts(&r).as_deref(), Some("impacts [], conflicts [{\"programId\":7}]"));
    assert!(impacts(&json!({"hasImpacts": true})).is_some());
    assert!(
        impacts(
            &json!({"extendedProfileResponse": {"applicantTypeProgramSelectionImpactResponse": {"allDelete": true}}})
        )
        .is_some()
    );
    assert_eq!(text(&json!({"question": "<p>All <b>applicants</b></p>"})), "All applicants");
    assert_eq!(readable(&json!({"input": {"type": "radio"}, "value": ["9"], "answerOptions": []})), None);
    // A program with nothing required and nothing to change: no finding.
    assert_eq!(plan(&[], "nursingcas", &[program(json!({"questionGroups": []}))]), (vec![], vec![]));
    // A stated impactRules is sent as it is.
    assert_eq!(body(&json!({"impactRules": {"x": 1}}), &[])["impactRules"], json!({"x": 1}));
    // A malformed document is passed through, not a panic.
    assert_eq!(body(&json!([1]), &[]), json!([1]));
    assert_eq!(
        body(&json!({"questionGroups": [1, {"questions": [2, {}]}]}), &[])["questionGroups"][1]["questions"][1]["value"],
        json!([""])
    );
}

#[test]
fn the_listing_shows_choices_and_open_keeps_only_required_blanks() {
    let fx = fixture();
    let all = listing(&[program(fx["before"].clone())], false);
    assert!(all.starts_with(&format!("\n{YALE}\n")), "{all}");
    assert!(
        all.contains(
            "  3113119 [required radio] Are you applying to a Joint Degree Program?\n      - \"No\"\n      - \"Yes\"\n"
        ),
        "{all}"
    );
    assert!(
        all.contains("  3113132 [required textarea, max 3500 chars] Personal Statement (3,000 characters)\n"),
        "{all}"
    );
    assert!(!all.contains("3113128"), "display-only text is not a question:\n{all}");
    let done = listing(&[program(fx["after"].clone())], false);
    assert!(done.contains("      = \"No\"\n"), "{done}");
    let open = listing(&[program(fx["after"].clone())], true);
    assert_eq!(
        open,
        format!("\n{YALE}\n  3113132 [required textarea, max 3500 chars] Personal Statement (3,000 characters)\n")
    );
    assert_eq!(listing(&[program(json!({"questionGroups": []}))], true), "");
}

#[test]
fn an_empty_answer_clears_a_typed_one() {
    // UC Davis, 2026-10-07 03:13:18: two typed answers cleared in one save.
    let fx: Value = serde_json::from_str(include_str!("fixtures/davis_questions.json")).expect("fixture");
    let davis = LiveProgram { id: 79437728, label: DAVIS.into(), doc: fx["after"].clone() };
    let (saves, _) = plan(
        &[ProgramAnswers {
            name: DAVIS.into(),
            portal: "nursingcas".into(),
            answers: [("3110249".to_string(), String::new())].into(),
        }],
        "nursingcas",
        &[davis],
    );
    assert_eq!(saves[0].changes[0].from.as_deref(), Some("LACCD"));
    let b = body(&saves[0].doc, &saves[0].changes);
    let q = questions(&b).find(|q| q["id"] == 3110249).expect("question");
    assert_eq!((&q["value"], &q["answerOptions"]), (&json!([""]), &json!([])));
}
