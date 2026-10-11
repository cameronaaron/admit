use super::*;

fn src() -> TranscriptSource {
    TranscriptSource {
        path: "t.pdf".into(),
        parser: "laccd".into(),
        colleges: [
            ("LAVC", "LOS ANGELES VALLEY COLLEGE"),
            ("LAPC", "LOS ANGELES PIERCE COLLEGE"),
            ("Connecticut College", "CONNECTICUT COLLEGE"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect(),
        summer_term: "Summer 2".into(),
        academic_status: "Post-Baccalaureate".into(),
        skip: vec!["TUTOR 001T".into()],
        subjects: [("ARTH C1200".to_string(), "Art History".to_string())].into_iter().collect(),
        create_missing_colleges: false,
        fix_titles: vec![],
    }
}

fn tc(code: &str, title: &str, college: &str, season: &str, grade: &str) -> TranscriptCourse {
    TranscriptCourse {
        year: "2026".into(),
        season: season.into(),
        code: code.into(),
        college: college.into(),
        title: title.into(),
        credits: "3.00".into(),
        grade: grade.into(),
        noncredit: false,
        term_months: None,
    }
}

fn lavc() -> LiveCollege {
    LiveCollege {
        id: 1,
        name: "LOS ANGELES VALLEY COLLEGE".into(),
        code: "001228".into(),
        terms: vec![
            json!({"id": 10, "term": "Fall", "year": "2026", "termInProgress": true, "courses": [
                {"id": 100, "code": "Biology 033", "title": "Medical Terminology", "subject": "Biology", "grade": ""}]}),
            json!({"id": 11, "term": "Summer 2", "year": "2026", "termInProgress": false, "courses": [
                {"id": 101, "code": "STAT 101", "title": "Statistics for the Social Sciences", "subject": "Statistics", "grade": "A"}]}),
        ],
        info: json!({"stillAttending": true, "collegeEndTerm": "Still Attending", "collegeEndYear": ""}),
    }
}

fn texts(f: &[Finding]) -> Vec<String> {
    f.iter().map(ToString::to_string).collect()
}

#[test]
fn matches_by_code_or_title_and_maps_summer_to_any_summer_term() {
    let courses = vec![
        tc("BIOLOGY 033", "Medical Terminology", "LAVC", "Fall", ""), // code differs only in case
        tc("STAT 101", "Stat For Social Sci", "LAVC", "Summer", "A"), // portal title expanded: a note
        tc("ARTH C1200", "Surv of Art", "LAVC", "Fall", ""),
    ];
    let p = plan(&src(), &courses, &[lavc()], |_| Some(1));
    let f = texts(&p.findings);
    assert!(f.len() == 1 && f[0].starts_with("note") && f[0].contains("STAT 101"), "{f:?}");
    assert_eq!(p.adds.len(), 1);
    assert_eq!(p.adds[0].target, TermTarget::Existing { id: 10, label: "Fall 2026".into() });
    assert_eq!(p.adds[0].subject, "Art History");
}

#[test]
fn missing_college_skip_list_subject_inference_and_posted_grades() {
    let courses = vec![
        tc("SOC 004", "Sociolog Analysis", "LAPC", "Fall", ""),
        tc("TUTOR 001T", "Supervised Tutoring", "LAVC", "Winter", "*"),
        tc("BIOLOGY 040", "Something", "LAVC", "Spring", "A"), // dept inferred from Biology 033
        tc("BIOLOGY 033", "Medical Terminology", "LAVC", "Fall", "A"), // grade posted
        tc("XYZ 1", "Unknown", "LAVC", "Spring", "A"),
    ];
    let p = plan(&src(), &courses, &[lavc()], |_| Some(1));
    let f = texts(&p.findings);
    assert!(f.iter().any(|m| m.contains("PIERCE COLLEGE is not on the application")), "{f:?}");
    assert!(f.iter().any(|m| m.contains("Biology 033") && m.contains("grade A has posted")), "{f:?}");
    assert!(f.iter().any(|m| m.starts_with("VIOLATION") && m.contains("XYZ 1")), "{f:?}");
    assert!(!f.iter().any(|m| m.contains("TUTOR")), "skipped: {f:?}");
    assert!(p.colleges.is_empty());
    assert_eq!(p.adds.len(), 1);
    assert_eq!(p.adds[0].subject, "Biology");
}

fn conn(courses: serde_json::Value) -> LiveCollege {
    LiveCollege {
        id: 2,
        name: "CONNECTICUT COLLEGE".into(),
        code: "001379".into(),
        terms: vec![courses],
        info: json!({"stillAttending": false, "collegeEndTerm": "Spring", "collegeEndYear": "2026"}),
    }
}

#[test]
fn a_retaken_course_matches_its_own_row_not_the_first_one() {
    // MAT 210: withdrawn in Fall, satisfactory (S*) in Spring.
    let live = LiveCollege {
        terms: vec![
            json!({"id": 1, "term": "Fall", "year": "2026", "termInProgress": false, "courses": [
                {"id": 7, "code": "MAT 210", "title": "DISCRETE MATHEMATICS", "subject": "Mathematics", "grade": "W"}]}),
            json!({"id": 2, "term": "Spring", "year": "2026", "termInProgress": false, "courses": [
                {"id": 8, "code": "MAT 210", "title": "DISCRETE MATHEMATICS", "subject": "Mathematics", "grade": "S*"}]}),
        ],
        ..conn(json!({}))
    };
    let courses = vec![
        tc("MAT 210", "DISCRETE MATHEMATICS", "Connecticut College", "Spring", "S*"),
        tc("MAT 210", "DISCRETE MATHEMATICS", "Connecticut College", "Fall", "W"),
    ];
    let p = plan(&src(), &courses, &[live], |_| Some(1));
    assert!(p.findings.is_empty() && p.adds.is_empty(), "{:?} {:?}", p.findings, p.adds);
}

#[test]
fn a_mistyped_code_is_reported_not_duplicated() {
    // The real case: the portal had PSY 323 "SPORT & EXERCISE" for PSY 321.
    let live = conn(json!({"id": 1, "term": "Spring", "year": "2026", "termInProgress": false, "courses": [
        {"id": 9, "code": "PSY 323", "title": "SPORT & EXERCISE", "subject": "Psychology", "grade": "B+"}]}));
    let courses = vec![tc("PSY 321", "SPORT & EXERCISE PSYCHOLOGY", "Connecticut College", "Spring", "B+")];
    let p = plan(&src(), &courses, &[live], |_| Some(1));
    assert!(p.adds.is_empty(), "never added as a duplicate");
    assert!(p.findings.is_empty(), "{:?}", texts(&p.findings));
    assert_eq!(p.edits.len(), 1);
    let e = &p.edits[0];
    assert_eq!((e.college_id, e.term_id, e.course_id), (2, 1, 9));
    assert_eq!(e.from, ("PSY 323".into(), "SPORT & EXERCISE".into()));
    assert_eq!(e.to, ("PSY 321".into(), "SPORT & EXERCISE PSYCHOLOGY".into()));
    // Everything but code and title is re-sent unchanged, in the create form's shape.
    assert_eq!(e.body["grade"], "B+");
    assert_eq!(e.body["gradeEnabled"], true);
    assert_eq!(e.body["termId"], "1");
    assert_eq!(e.body["credits"], "");
    assert_eq!(e.body["courseClassification"]["labSelected"], false);
}

fn college(name: &str, info: serde_json::Value) -> LiveCollege {
    LiveCollege { id: 5, name: name.into(), code: String::new(), terms: vec![], info }
}

#[test]
fn still_attending_follows_ungraded_courses_on_the_transcript() {
    let courses = vec![
        tc("SOC 004", "Sociolog Analysis", "LAPC", "Fall", ""),
        tc("STAT 101", "Stat For Social Sci", "LAVC", "Summer", "A"),
    ];
    // Pierce: has an ungraded Fall course but is marked as ended.
    let pierce = college(
        "LOS ANGELES PIERCE COLLEGE",
        json!({"stillAttending": false, "collegeEndTerm": "Fall", "collegeEndYear": "2026"}),
    );
    // Valley: all graded (Summer 2026) but marked still attending.
    let valley = college(
        "LOS ANGELES VALLEY COLLEGE",
        json!({"stillAttending": true, "collegeEndTerm": "Still Attending", "collegeEndYear": ""}),
    );
    let p = plan(&src(), &courses, &[pierce, valley], |_| Some(1));
    let a: Vec<(String, bool, (String, String), u8)> =
        p.attendance.iter().map(|f| (f.college.clone(), f.still_attending, f.last.clone(), f.end_month)).collect();
    assert_eq!(
        a,
        vec![
            ("LOS ANGELES PIERCE COLLEGE".into(), true, ("Fall".into(), "2026".into()), 12),
            ("LOS ANGELES VALLEY COLLEGE".into(), false, ("Summer".into(), "2026".into()), 8),
        ]
    );
}

#[test]
fn attendance_already_right_is_left_alone() {
    let courses = vec![tc("STAT 101", "Stat For Social Sci", "LAVC", "Summer", "A")];
    let ended_right = college(
        "LOS ANGELES VALLEY COLLEGE",
        json!({"stillAttending": false, "collegeEndTerm": "Summer 2", "collegeEndYear": "2026"}),
    );
    assert!(plan(&src(), &courses, &[ended_right], |_| Some(1)).attendance.is_empty());
    let ended_early = college(
        "LOS ANGELES VALLEY COLLEGE",
        json!({"stillAttending": false, "collegeEndTerm": "Spring", "collegeEndYear": "2026"}),
    );
    assert_eq!(
        plan(&src(), &courses, &[ended_early], |_| Some(1)).attendance.len(),
        1,
        "end term behind the transcript"
    );
}

#[test]
fn missing_college_is_planned_from_its_coursework_when_allowed() {
    let mut sp = src();
    sp.create_missing_colleges = true;
    let mut fall = tc("SOC 004", "Sociolog Analysis", "LAPC", "Fall", "");
    fall.term_months = Some((9, 12));
    let mut spring = tc("SOC 001", "Intro", "LAPC", "Spring", "A");
    spring.term_months = Some((2, 6));
    let p = plan(&sp, &[fall, spring], &[lavc()], |_| Some(1));
    assert_eq!(
        p.colleges,
        vec![CollegeCreate {
            name: "LOS ANGELES PIERCE COLLEGE".into(),
            first: ("Spring".into(), "2026".into()),
            last: ("Fall".into(), "2026".into()),
            months: (2, 12),
            still_attending: true,
        }]
    );
    assert!(p.findings.is_empty(), "{:?}", p.findings);
}

#[test]
fn subjects_named_in_the_profile_are_corrected_on_matched_courses() {
    // Medical Terminology was filed under Biology, a science subject.
    let mut src = src();
    src.subjects.insert("BIOLOGY 033".into(), "Medical Terminology".into());
    let courses = vec![tc("BIOLOGY 033", "Medical Terminology", "LAVC", "Fall", "")];
    let ids = |name: &str| match name {
        "Medical Terminology" => Some(4700),
        "Biology" => Some(4527),
        _ => None,
    };
    let p = plan(&src, &courses, &[lavc()], ids);
    assert!(p.adds.is_empty());
    assert_eq!(p.edits.len(), 1, "{:?}", texts(&p.findings));
    let e = &p.edits[0];
    assert_eq!(e.subject, Some(("Biology".into(), "Medical Terminology".into())));
    assert_eq!(e.to, ("Biology 033".into(), "Medical Terminology".into()), "code and title kept");
    assert_eq!(e.body["subjectId"], "4700");
    // Already right: nothing to do.
    src.subjects.insert("BIOLOGY 033".into(), "Biology".into());
    assert!(plan(&src, &courses, &[lavc()], ids).edits.is_empty());
    // A subject the portal doesn't offer is a violation, not an edit.
    src.subjects.insert("BIOLOGY 033".into(), "Biologee".into());
    let p = plan(&src, &courses, &[lavc()], ids);
    assert!(p.edits.is_empty());
    assert!(texts(&p.findings).iter().any(|f| f.starts_with("VIOLATION") && f.contains("Biologee")));
}

#[test]
fn a_code_missing_its_number_is_corrected_from_the_transcript() {
    // The real case: ELAC "POL SCI" for "POL SCI 019", matched by title.
    let live = LiveCollege {
        terms: vec![json!({"id": 3, "term": "Summer 2", "year": "2026", "termInProgress": false, "courses": [
            {"id": 30, "code": "POL SCI", "title": "Women in Politics", "subject": "Political Science",
             "subjectId": 4800, "grade": "A", "credits": "3.00"}]})],
        ..lavc()
    };
    let courses = vec![tc("POL SCI 019", "Women in Politics", "LAVC", "Summer", "A")];
    let p = plan(&src(), &courses, &[live], |_| Some(1));
    assert_eq!(p.edits.len(), 1);
    assert_eq!(p.edits[0].to, ("POL SCI 019".into(), "Women in Politics".into()));
    assert_eq!(p.edits[0].subject, None);
    assert_eq!(p.edits[0].body["subjectId"], "4800", "subject unchanged");
}

#[test]
fn titles_listed_in_fix_titles_take_the_transcripts_wording() {
    let live = conn(json!({"id": 1, "term": "Spring", "year": "2026", "termInProgress": false, "courses": [
        {"id": 9, "code": "PSY 309", "title": "BEHAVIORAL MEDECINE", "subject": "Psychology", "grade": "B"},
        {"id": 10, "code": "COM 110", "title": "INTRO TO COMPUTER SCIENCE/PROB SOLV", "subject": "Computer Science", "grade": "A"}]}));
    let courses = vec![
        tc("PSY 309", "BEHAVIORAL MEDICINE", "Connecticut College", "Spring", "B"),
        tc("COM 110", "INTRO COMPUTER SCI/PROB SOLV", "Connecticut College", "Spring", "A"),
    ];
    let mut src = src();
    // Unlisted: the misspelling is still caught (word-level fix); the
    // abbreviation (COM 110) is never edited.
    let p = plan(&src, &courses, std::slice::from_ref(&live), |_| Some(1));
    assert_eq!(p.edits.len(), 1, "only the typo");
    assert_eq!(p.edits[0].to.1, "BEHAVIORAL MEDICINE");
    src.fix_titles = vec!["PSY 309".into()];
    let p = plan(&src, &courses, &[live], |_| Some(1));
    assert_eq!(p.edits.len(), 1, "only the listed course");
    assert_eq!(p.edits[0].to, ("PSY 309".into(), "BEHAVIORAL MEDICINE".into()));
    assert!(!texts(&p.findings).iter().any(|f| f.contains("MEDECINE")), "no leftover note");
}

#[test]
fn misspellings_are_caught_but_abbreviations_are_not() {
    // Every typo found on the portal on 2026-10-06.
    let cases = [
        ("CONCEPTIONS OF GIFTEDNEDD", "Conceptions of Giftedness", vec![("GIFTEDNEDD", "GIFTEDNESS")]),
        ("ILLIMINATING DISEASE", "ILLUMINATING DISEASE", vec![("ILLIMINATING", "ILLUMINATING")]),
        ("CC: SUPERHEROS & UNDERDOGS", "CC: SUPERHEROES & UNDERDOGS", vec![("SUPERHEROS", "SUPERHEROES")]),
        ("BEHAVIORAL MEDECINE", "BEHAVIORAL MEDICINE", vec![("MEDECINE", "MEDICINE")]),
    ];
    for (portal, transcript, want) in cases {
        let got = title_typos(portal, transcript);
        let want: Vec<(String, String)> = want.into_iter().map(|(a, b)| (a.into(), b.into())).collect();
        assert_eq!(got, want, "{portal}");
    }
    // Abbreviations and rewordings seen on the same transcripts: no typo.
    for (portal, transcript) in [
        ("PSYCHOLOGY DISORDERS/DYSFUNCTION", "PSYCHOLOG DISORDERS/DYSFUNCT"),
        ("INTRO TO COMPUTER SCIENCE/PROB SOLV", "INTRO COMPUTER SCI/PROB SOLV"),
        ("HISTORY ARTS & TECHNOLOGY", "HIST OF ARTS & TECHNOLOGY"),
        ("METHODS AND TECHNIQUES OF ED RESEARCH PT 1", "Methods and Techniques of Educational Research Part 1"),
        ("INTRO TO HUMAN PHYS", "Intro to Human Physiology"),
        ("Statistics for the Social Sciences", "Stat For Social Sci"),
        // Valley's transcript drops vowels: the portal's spelling is right.
        ("COMMUNICATION SKILLS", "Communication Sklls"),
    ] {
        assert!(title_typos(portal, transcript).is_empty(), "{portal} vs {transcript}");
    }
    assert_eq!(
        title_typos("Conceptions of Giftednedd", "CONCEPTIONS OF GIFTEDNESS")[0].1,
        "Giftedness",
        "portal case kept"
    );
}

#[test]
fn a_typo_is_a_violation_and_an_edit_of_only_that_word() {
    let live = conn(json!({"id": 1, "term": "Spring", "year": "2026", "termInProgress": false, "courses": [
        {"id": 9, "code": "PSY 309", "title": "BEHAVIORAL MEDECINE (HONORS)", "subject": "Psychology", "grade": "B"}]}));
    let courses = vec![tc("PSY 309", "BEHAVIORAL MEDICINE", "Connecticut College", "Spring", "B")];
    let p = plan(&src(), &courses, &[live], |_| Some(1));
    assert_eq!(p.edits.len(), 1);
    assert_eq!(p.edits[0].to.1, "BEHAVIORAL MEDICINE (HONORS)", "the rest of the portal title stays");
    assert!(texts(&p.findings).iter().any(|f| f.starts_with("VIOLATION") && f.contains("\"MEDECINE\"")));
}
