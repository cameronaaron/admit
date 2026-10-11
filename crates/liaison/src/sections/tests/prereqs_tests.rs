use admit_domain::{PrerequisitesFile, Requirement};

use super::*;

const YALE: &str =
    "Yale University - Graduate Entry Prespecialty in Nursing (GEPN) / Master of Science in Nursing (MSN) Program";

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/yale_prereqs.json")).expect("fixture")
}

fn colleges() -> BTreeMap<String, String> {
    [
        ("LACC", "LOS ANGELES CITY COLLEGE"),
        ("LAHC", "LOS ANGELES HARBOR COLLEGE"),
        ("LAVC", "LOS ANGELES VALLEY COLLEGE"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

fn req(name: &str, college: &str, code: &str) -> Requirement {
    Requirement {
        name: name.into(),
        courses: vec![CourseRef { college: college.into(), code: code.into() }],
        within_years: None,
        deferred: None,
    }
}

fn ruling(name: &str, requirements: Vec<Requirement>) -> PrerequisiteProgram {
    PrerequisiteProgram {
        name: name.into(),
        portal: Some("nursingcas".into()),
        start: None,
        min_grade: None,
        source: None,
        requirements,
    }
}

/// The rulings the applicant made by hand on 2026-10-07, in the order clicked.
fn yale_ruling() -> PrerequisiteProgram {
    ruling(
        YALE,
        vec![
            req("Human Anatomy & Physiology I or Anatomy w/Lab", "LACC", "ANATOMY 1"),
            req("Human Anatomy & Physiology II or Physiology w/Lab", "LAHC", "PHYSIOL 001"),
            req("General Chemistry w/Lab", "LAVC", "CHEM 051"),
            req("Microbiology w/Lab", "LACC", "MICRO 020"),
        ],
    )
}

fn yale_live(doc: Value) -> Vec<LiveProgram> {
    vec![LiveProgram { id: 79439764, label: YALE.into(), doc }]
}

#[test]
fn planned_posts_are_the_ones_the_form_sent_byte_for_byte() {
    let fx = fixture();
    let pool = pool(&fx["transcripts"]);
    assert_eq!(pool.len(), 4);
    let (assigns, f) = plan(&[yale_ruling()], "nursingcas", &colleges(), &yale_live(fx["get"].clone()), &pool);
    assert!(f.is_empty(), "{f:?}");
    let codes: Vec<&str> = assigns.iter().map(|a| a.course.code.as_str()).collect();
    assert_eq!(codes, ["ANATOMY 001", "PHYSIOL 001", "CHEM 051", "MICRO 020"], "\"ANATOMY 1\" matches ANATOMY 001");
    // Each POST builds on the previous response, as the form's did.
    let mut doc = fx["get"].clone();
    for (a, sent) in assigns.iter().zip(arr(&fx["posts"])) {
        assert_eq!(body(&doc, a), sent["request"], "{a}");
        doc = sent["response"].clone();
    }
    // Once all four are on the portal, a re-plan (of the GET, which has the
    // names a POST response leaves null) has nothing left to assign.
    assert!(arr(&doc["prerequisites"]).iter().all(|q| q["name"].is_null()), "captured: responses drop names");
    let mut reread = fx["get"].clone();
    for (q, done) in reread["prerequisites"].as_array_mut().expect("reqs").iter_mut().zip(arr(&doc["prerequisites"])) {
        q["course"] = done["course"].clone();
    }
    let (again, f) = plan(&[yale_ruling()], "nursingcas", &colleges(), &yale_live(reread), &pool);
    assert!(again.is_empty() && f.is_empty(), "{again:?} {f:?}");
    assert_eq!(
        assigns[0].to_string(),
        format!(
            "{YALE}: Human Anatomy & Physiology I or Anatomy w/Lab <- LOS ANGELES CITY COLLEGE ANATOMY 001 (INTRO TO HUMAN ANATOMY Spring 2026)"
        )
    );
}

#[test]
fn rulings_that_name_nothing_on_the_portal_are_violations_and_gaps_are_pending() {
    let fx = fixture();
    let mut pool = pool(&fx["transcripts"]);
    let mut retake = pool[0].clone();
    retake.id = 1;
    retake.year = "2027".into();
    pool.push(retake);
    let mut deferred = req("General Chemistry w/Lab", "LAVC", "NOPE 1");
    deferred.deferred = Some("may be completed after admission".into());
    let rulings = [
        ruling(
            YALE,
            vec![
                req("Human Anatomy & Physiology I or Anatomy w/Lab", "LACC", "ANATOMY 001"),
                req("Human Anatomy & Physiology II or Physiology w/Lab", "LAHC", "PHYSIOL 999"),
                req("Not A Requirement", "LACC", "MICRO 020"),
                deferred,
            ],
        ),
        ruling("Nowhere - Nothing", vec![]),
        PrerequisiteProgram { portal: Some("other".into()), ..ruling(YALE, vec![]) },
    ];
    let mut doc = fx["get"].clone();
    // Microbiology already has a course the profile doesn't name.
    doc["prerequisites"][3]["course"] = json!([{"id": 9}]);
    let mut live = yale_live(doc);
    live.push(LiveProgram {
        id: 2,
        label: "Westmont College - ABSN".into(),
        doc: json!({"prerequisites": [{"id": 1, "name": "Chemistry", "course": []}]}),
    });
    let (assigns, f) = plan(&rulings, "nursingcas", &colleges(), &live, &pool);
    assert!(assigns.is_empty(), "{assigns:?}");
    let text: Vec<String> = f.iter().map(ToString::to_string).collect();
    let want = [
        "VIOLATION  prerequisites.toml: \"Nowhere - Nothing\" is not a program with prerequisites on nursingcas".to_string(),
        format!("VIOLATION  {YALE}: Human Anatomy & Physiology I or Anatomy w/Lab: LACC ANATOMY 001 matches 2 entered courses (Spring 2026, Spring 2027); prerequisites can't tell which"),
        format!("VIOLATION  {YALE}: Human Anatomy & Physiology II or Physiology w/Lab: LAHC PHYSIOL 999 is not entered on the portal under LOS ANGELES HARBOR COLLEGE"),
        format!("VIOLATION  {YALE}: no prerequisite named \"Not A Requirement\" on the portal"),
        format!("note       {YALE}: prerequisite \"Microbiology w/Lab\" is assigned on the portal only"),
        "PENDING    Westmont College - ABSN: no prerequisite rulings in prerequisites.toml (`admit -p nursingcas prereqs` prints its requirements)".to_string(),
    ];
    assert_eq!(text, want);

    // A requirement left out of a ruling with nothing assigned is still to do.
    let partial = ruling(YALE, vec![req("Microbiology w/Lab", "LACC", "MICRO 020")]);
    let (_, f) = plan(&[partial], "nursingcas", &colleges(), &yale_live(fixture()["get"].clone()), &pool);
    assert!(
        f.contains(&Finding::Pending(format!(
            "{YALE}: prerequisite \"General Chemistry w/Lab\" has no course and no ruling"
        ))),
        "{f:?}"
    );
    // An unmapped college label is taken as the portal's own name.
    assert!(
        resolve(
            &CourseRef { college: "LOS ANGELES CITY COLLEGE".into(), code: "MICRO 20".into() },
            &BTreeMap::new(),
            &pool
        )
        .is_ok()
    );
}

#[test]
fn printed_rulings_read_back_and_plan_nothing_for_what_is_assigned() {
    let fx = fixture();
    let done = arr(&fx["posts"]).last().expect("posts")["response"].clone();
    let mut doc = done.clone();
    for (q, g) in doc["prerequisites"].as_array_mut().expect("reqs").iter_mut().zip(arr(&fx["get"]["prerequisites"])) {
        q["name"] = g["name"].clone();
    }
    doc["prerequisites"][0]["minGradeAlphabetic"] = json!("");
    let live = yale_live(doc);
    let text = to_toml(&live, "nursingcas", &colleges(), &[]);
    assert!(text.contains("courses = [{ college = \"LACC\", code = \"ANATOMY 001\" }]"), "{text}");
    assert!(
        text.contains("# min grade unstated, 4 credit(s)") && text.contains("# min grade B, 4 credit(s)"),
        "{text}"
    );
    let file: PrerequisitesFile = toml::from_str(&text).expect("valid TOML");
    let (assigns, f) = plan(&file.programs, "nursingcas", &colleges(), &live, &pool(&fx["transcripts"]));
    assert!(assigns.is_empty() && f.is_empty(), "{assigns:?} {f:?}");
    assert_eq!(to_toml(&live, "nursingcas", &colleges(), &file.programs), "", "a ruled program is not printed again");
}

#[test]
fn resent_courses_keep_unparseable_credits_and_numbers_follow_javascript() {
    let c = resend(&json!({"id": 1, "gradeValue": "n/a", "grade": null}));
    assert_eq!(c["gradeValue"], "n/a");
    // A malformed document is passed through, not a panic.
    let a = Assign {
        program_id: 1,
        program: "P".into(),
        requirement_id: 1,
        requirement: "R".into(),
        course: pool(&fixture()["transcripts"])[0].clone(),
    };
    assert_eq!(body(&json!([1]), &a), json!([1]));
    assert_eq!(body(&json!({"prerequisites": [7]}), &a), json!({"prerequisites": [7]}));
    let mut v = json!({"a": [3.0, 2.5, 7], "b": 1e300});
    js_numbers(&mut v);
    assert_eq!(v, json!({"a": [3, 2.5, 7], "b": 1e300}));
}

#[test]
fn ruled_courses_short_of_the_minimum_credits_are_pending() {
    // Live 2026-10-07: California Baptist asks 4 credits of COMM C1000's 3.
    let fx = fixture();
    let pool = pool(&fx["transcripts"]);
    let mut doc = fx["get"].clone();
    doc["prerequisites"][0]["minCredits"] = json!(5);
    let mut two = req("Microbiology w/Lab", "LACC", "MICRO 020");
    two.courses.push(CourseRef { college: "LAHC".into(), code: "PHYSIOL 001".into() });
    doc["prerequisites"][3]["minCredits"] = json!(8);
    let missing = req("General Chemistry w/Lab", "LAVC", "CHEM 999");
    let rulings =
        [ruling(YALE, vec![req("Human Anatomy & Physiology I or Anatomy w/Lab", "LACC", "ANATOMY 001"), two, missing])];
    let (_, f) = plan(&rulings, "nursingcas", &colleges(), &yale_live(doc), &pool);
    let pending: Vec<&str> = f.iter().filter(|x| matches!(x, Finding::Pending(_))).map(Finding::message).collect();
    assert_eq!(
        pending,
        [
            format!(
                "{YALE}: Human Anatomy & Physiology I or Anatomy w/Lab: the ruled course(s) total 4 credit(s); the program asks for 5"
            ),
            format!(
                "{YALE}: prerequisite \"Human Anatomy & Physiology II or Physiology w/Lab\" has no course and no ruling"
            ),
        ],
        "two courses sum to 8 and meet 8; an unresolved course skips the check: {f:?}"
    );
}
