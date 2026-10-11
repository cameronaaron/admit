use super::*;
#[test]
fn invalid_programs_and_unknown_term_shapes_have_specific_findings() {
    let mut p = PrerequisiteProgram {
        name: "Test".into(),
        portal: None,
        start: Some("invalid".into()),
        min_grade: None,
        source: None,
        requirements: vec![],
    };
    let t = Transcripts { courses: vec![], unreadable: BTreeMap::new() };
    let today = time::macros::date!(2026 - 10 - 06);
    assert!(check(&p, &t, today)[0].to_string().contains("not YYYY-MM"));
    p.start = Some("2027-01".into());
    p.min_grade = Some("Pass".into());
    assert!(check(&p, &t, today)[0].to_string().contains("not a letter grade"));
    p.min_grade = None;
    p.requirements.push(Requirement { name: "Science".into(), courses: vec![], within_years: None, deferred: None });
    assert!(check(&p, &t, today)[0].to_string().contains("no course counted"));
    assert_eq!(points("B-"), Some(2.7));
    assert_eq!(points("B++"), None);
    let x = TranscriptCourse {
        year: "2026".into(),
        season: "Unknown".into(),
        code: "SCI 1".into(),
        college: "COL".into(),
        title: "Science".into(),
        credits: "3.00".into(),
        grade: "".into(),
        noncredit: false,
        term_months: Some((2, 6)),
    };
    assert_eq!(term_end(&x), Some(2026 * 12 + 5));
    assert_eq!(term_end(&TranscriptCourse { term_months: None, ..x.clone() }), None);
    let winter = TranscriptCourse { season: "Winter".into(), term_months: None, ..x.clone() };
    assert_eq!(term_end(&winter), Some(2026 * 12 + 1));
    assert_eq!(term_end(&TranscriptCourse { year: "nope".into(), ..winter }), None);
    assert_eq!(points(""), None);
    assert_eq!(points("A+"), Some(4.3));
    assert_eq!(month("2027-13"), None);
    assert_eq!(month("yyyy-01"), None);
    assert_eq!(month("2027-ab"), None);
    assert_eq!(month("2027"), None);
    assert_eq!(worst(std::iter::empty()), Level::Done);
    assert_eq!(worst([Level::Done, Level::Failed].into_iter()), Level::Failed);
}

#[test]
fn unread_and_known_colleges_and_a_term_without_an_end() {
    let dir = std::env::temp_dir().join(format!("admit-prereq-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("coursework.toml"),
        "[[transcript]]\npath='missing.pdf'\nparser='laccd'\nsummer_term='Summer'\nacademic_status='Freshman'\n[transcript.colleges]\nLAVC='School'\nELAC='Other'\n",
    )
    .expect("coursework");
    std::fs::write(
        dir.join("prerequisites.toml"),
        "[[program]]\nname='Test'\nstart='2027-09'\nportal='ucla'\n[[program.requirement]]\nname='Bio'\n[[program.requirement.courses]]\ncollege='LAVC'\ncode='BIO 1'\n[[program.requirement.courses]]\ncollege='Nope'\ncode='X 1'\n[[program]]\nname='Other'\nstart='2027-09'\n[[program.requirement]]\nname='X'\n",
    )
    .expect("prereq");
    let profile = Profile::load(&dir).expect("profile");
    let unread = read_transcripts(&profile, &dir);
    assert_eq!(unread.unreadable.len(), 2, "{:?}", unread.unreadable);
    assert!(unread.courses.is_empty());
    assert!(unknown_colleges(&profile).contains("Nope"));
    let today = time::macros::date!(2026 - 10 - 06);
    assert!(findings(&profile, &dir, Some("missing"), today).is_empty());
    let found = findings(&profile, &dir, Some("ucla"), today);
    assert!(found.iter().any(|f| f.to_string().contains("unchecked")), "{found:?}");
    assert!(findings(&profile, &dir, None, today).iter().any(|f| f.to_string().contains("no course")));
    let mystery = Requirement {
        name: "Science".into(),
        courses: vec![CourseRef { college: "COL".into(), code: "SCI 1".into() }],
        within_years: Some(5),
        deferred: None,
    };
    let course = TranscriptCourse {
        year: "2026".into(),
        season: "Mystery".into(),
        code: "SCI 1".into(),
        college: "COL".into(),
        title: "Science".into(),
        credits: "3".into(),
        grade: "A".into(),
        noncredit: false,
        term_months: None,
    };
    let program = PrerequisiteProgram {
        name: "Test".into(),
        portal: None,
        start: Some("2027-09".into()),
        min_grade: None,
        source: None,
        requirements: vec![mystery.clone()],
    };
    let note = check(&program, &Transcripts { courses: vec![course], unreadable: BTreeMap::new() }, today);
    assert!(note[0].to_string().contains("met by"), "{note:?}");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn a_program_without_a_start_is_checked_unless_it_has_a_recency_limit() {
    // NursingCAS states no start; its rulings come from the portal.
    let req = |within_years| Requirement {
        name: "Anatomy".into(),
        courses: vec![CourseRef { college: "LACC".into(), code: "ANATOMY 001".into() }],
        within_years,
        deferred: None,
    };
    let mut p = PrerequisiteProgram {
        name: "Yale - GEPN".into(),
        portal: Some("nursingcas".into()),
        start: None,
        min_grade: None,
        source: None,
        requirements: vec![req(None)],
    };
    let t = Transcripts { courses: vec![], unreadable: BTreeMap::new() };
    let today = time::macros::date!(2026 - 10 - 07);
    let f = check(&p, &t, today);
    assert_eq!(f.len(), 1);
    assert!(!f[0].message().contains("start"), "checked without a start: {f:?}");
    p.requirements.push(req(Some(5)));
    assert_eq!(
        check(&p, &t, today),
        vec![Finding::Violation("Yale - GEPN: within_years needs the program's start (YYYY-MM)".into())]
    );
}
