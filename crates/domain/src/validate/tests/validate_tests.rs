use time::macros::date;

use super::*;
use crate::*;

fn exp(title: &str, start: &str, end: Option<&str>, hpw: u32) -> Experience {
    Experience {
        kind: ExperienceKind::Employment,
        organization: "Org".into(),
        title: title.into(),
        state: "California".into(),
        country: "United States".into(),
        start: start.into(),
        end: end.map(Into::into),
        current: end.is_none(),
        status: "Part time".into(),
        hours_per_week: hpw,
        weeks: 10,
        total_hours: hpw * 10,
        compensated: true,
        volunteer: false,
        description: "x".into(),
        supervisor: Supervisor::default(),
        ids: PortalIds::new(),
    }
}

fn with_experiences(xs: Vec<Experience>) -> Profile {
    Profile { experiences: ExperiencesFile { experiences: xs }, ..Default::default() }
}

const TODAY: Date = date!(2026 - 10 - 06);

#[test]
fn overlapping_roles_over_the_limit_are_one_violation_per_run() {
    // The real 2018-19 Connecticut College case: three 40h roles at once.
    let p = with_experiences(vec![
        exp("Lab RA", "2017-08-01", Some("2021-05-01"), 40),
        exp("AI Associate", "2018-07-01", Some("2019-05-01"), 40),
        exp("Lab Manager", "2018-08-01", Some("2019-05-01"), 40),
    ]);
    let v: Vec<_> = validate(&p, TODAY).into_iter().filter(Finding::is_violation).collect();
    assert_eq!(v.len(), 1, "{v:?}");
    let msg = v[0].to_string();
    assert!(msg.contains("August 2018 – April 2019") && msg.contains("120 h/week"), "{msg}");
}

#[test]
fn a_single_over_limit_month_names_only_that_month() {
    let p = with_experiences(vec![
        exp("Day", "2020-01-15", Some("2020-01-31"), 50),
        exp("Night", "2020-01-15", Some("2020-01-31"), 50),
    ]);
    let v: Vec<String> = validate(&p, TODAY).into_iter().filter(Finding::is_violation).map(|f| f.to_string()).collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("January 2020: overlapping") && v[0].contains("100 h/week"), "{v:?}");
    assert!(!v[0].contains('–'), "{v:?}");
}

#[test]
fn back_to_back_roles_do_not_overlap() {
    // Ends on the 1st of the month the next one starts: no shared month.
    let p = with_experiences(vec![
        exp("A", "2021-07-01", Some("2021-08-01"), 60),
        exp("B", "2021-08-01", Some("2022-02-01"), 60),
    ]);
    assert!(!validate(&p, TODAY).iter().any(Finding::is_violation));
}

#[test]
fn missing_patient_care_is_pending() {
    let p = with_experiences(vec![exp("A", "2020-01-01", Some("2020-06-01"), 10)]);
    assert!(validate(&p, TODAY).iter().any(|f| matches!(f, Finding::Pending(m) if m.contains("patient-care"))));
}

#[test]
fn degree_status_must_agree_with_date() {
    let degree = |status, month: &str, year| Degree {
        kind: "Associate of Arts".into(),
        major: format!("{status:?} {month} {year}"),
        second_major: None,
        minor: None,
        status,
        month: month.into(),
        year,
        note: None,
    };
    let p = Profile {
        education: EducationFile {
            colleges: vec![College {
                name: "X".into(),
                degrees: vec![
                    degree(DegreeStatus::Expected, "September", 2026), // past: rejected by portal
                    degree(DegreeStatus::Expected, "December", 2026),  // fine
                    degree(DegreeStatus::Awarded, "August", 2026),     // fine
                    degree(DegreeStatus::Awarded, "December", 2026),   // future award
                ],
            }],
        },
        ..Default::default()
    };
    let v: Vec<String> = validate(&p, TODAY).into_iter().filter(Finding::is_violation).map(|f| f.to_string()).collect();
    assert_eq!(v.len(), 2, "{v:?}");
    assert!(v[0].contains("September 2026 is not in the future") && v[1].contains("December 2026 is in the future"));
}

#[test]
fn high_school_dates_must_be_past_and_ordered() {
    // The real mistake: attendance recorded as ending September 2027.
    let p = Profile {
        high_school: HighSchoolFile {
            high_schools: vec![HighSchool {
                name: "BRIDGES ACADEMY".into(),
                from: "2013-09".into(),
                to: "2027-09".into(),
                diploma: "High School Diploma".into(),
                diploma_date: "2017-05".into(),
                term_type: None,
            }],
        },
        ..Default::default()
    };
    let v: Vec<String> = validate(&p, TODAY).iter().map(ToString::to_string).collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("to is in the future"));
}

#[test]
fn high_school_year_and_month_must_both_parse() {
    let school = |from: &str, to: &str| HighSchool {
        name: "X".into(),
        from: from.into(),
        to: to.into(),
        diploma: "Diploma".into(),
        diploma_date: "2017-06".into(),
        term_type: None,
    };
    let messages = |from, to| {
        let p = Profile { high_school: HighSchoolFile { high_schools: vec![school(from, to)] }, ..Default::default() };
        validate(&p, TODAY).iter().map(ToString::to_string).collect::<Vec<_>>()
    };
    let bad_year = messages("abcd-01", "2017-06");
    assert!(bad_year.iter().any(|m| m.contains("from \"abcd-01\"")), "{bad_year:?}");
    let bad_month = messages("2017-06", "2020-xx");
    assert!(bad_month.iter().any(|m| m.contains("to \"2020-xx\"")), "{bad_month:?}");
    // No '-' at all: the split in year_month fails before either number parses.
    let no_dash = messages("nodash", "2017-06");
    assert!(no_dash.iter().any(|m| m.contains("from \"nodash\"")), "{no_dash:?}");
}

#[test]
fn a_bad_degree_month_is_not_compared_with_the_diploma() {
    let mut p = Profile::default();
    p.high_school.high_schools.push(HighSchool {
        name: "Bridges".into(),
        from: "2013-09".into(),
        to: "2017-06".into(),
        diploma: "Diploma".into(),
        diploma_date: "2017-06".into(),
        term_type: None,
    });
    p.education.colleges.push(College {
        name: "X".into(),
        degrees: vec![Degree {
            kind: "Bachelor of Arts".into(),
            major: "Art".into(),
            second_major: None,
            minor: None,
            status: DegreeStatus::Awarded,
            month: "Smarch".into(),
            year: 2016,
            note: None,
        }],
    });
    let v: Vec<String> = validate(&p, TODAY).iter().map(ToString::to_string).collect();
    assert!(v.iter().any(|m| m.contains("month \"Smarch\" is not a month name")), "{v:?}");
    assert!(!v.iter().any(|m| m.contains("before the")), "{v:?}");
}

#[test]
fn licenses_without_a_date_or_with_a_bad_date_are_reported() {
    let mut p = Profile::default();
    for (title, until) in [("Open", None), ("Broken", Some("nope"))] {
        p.licenses.licenses.push(License {
            kind: LicenseKind::Certification,
            title: title.into(),
            organization: "Board".into(),
            description: None,
            valid_until: until.map(Into::into),
            ids: Default::default(),
        });
    }
    let v: Vec<String> = validate(&p, TODAY).iter().map(ToString::to_string).collect();
    assert!(v.iter().any(|m| m.contains("\"Broken\"") && m.contains("not YYYY-MM-DD")), "{v:?}");
    assert!(!v.iter().any(|m| m.contains("Open")), "{v:?}");
}

#[test]
fn achievement_dates_may_be_missing_invalid_or_already_past() {
    let ach = |name: &str, date: Option<&str>| Achievement {
        kind: AchievementKind::Honor,
        name: name.into(),
        organization: None,
        date: date.map(Into::into),
        state: "California".into(),
        country: "United States".into(),
        description: None,
        ids: PortalIds::new(),
    };
    let p = Profile {
        achievements: AchievementsFile {
            achievements: vec![ach("  ", None), ach("Named", Some("bad")), ach("Past", Some("2020-01-01"))],
        },
        ..Default::default()
    };
    let v: Vec<String> = validate(&p, TODAY).iter().map(ToString::to_string).collect();
    assert!(v.iter().any(|m| m.contains("no name")), "{v:?}");
    assert!(v.iter().any(|m| m.contains("not YYYY-MM-DD")), "{v:?}");
    assert!(!v.iter().any(|m| m.contains("Past")), "{v:?}");
    let note = Finding::Note("kept".into()).to_string();
    assert!(note.starts_with("note") && note.contains("kept"));
}

#[test]
fn achievement_limits_and_future_dates() {
    let p = Profile {
        achievements: AchievementsFile {
            achievements: vec![Achievement {
                kind: AchievementKind::Honor,
                name: "x".repeat(61),
                organization: None,
                date: Some("2027-01-01".into()),
                state: "California".into(),
                country: "United States".into(),
                description: Some("y".repeat(600)),
                ids: PortalIds::new(),
            }],
        },
        ..Default::default()
    };
    let v: Vec<String> = validate(&p, TODAY).iter().map(ToString::to_string).collect();
    assert!(v.iter().any(|m| m.contains("name is 61 characters")), "{v:?}");
    assert!(v.iter().any(|m| m.contains("in the future")), "{v:?}");
    assert!(!v.iter().any(|m| m.contains("description")), "600 is allowed: {v:?}");
}

#[test]
fn licenses_expired_or_expiring_soon_are_flagged() {
    let mut p = Profile::default();
    for (t, until) in [("BLS", "2026-01-31"), ("ACLS", "2026-12-31"), ("PALS", "2028-01-31")] {
        p.licenses.licenses.push(crate::License {
            kind: crate::LicenseKind::Certification,
            title: t.into(),
            organization: "AHA".into(),
            description: None,
            valid_until: Some(until.into()),
            ids: Default::default(),
        });
    }
    let f: Vec<String> = validate(&p, TODAY).iter().map(ToString::to_string).collect();
    assert!(f.iter().any(|x| x.starts_with("VIOLATION") && x.contains("\"BLS\": expired")), "{f:?}");
    assert!(f.iter().any(|x| x.starts_with("PENDING") && x.contains("\"ACLS\": expires 2026-12-31")), "{f:?}");
    assert!(!f.iter().any(|x| x.contains("PALS")), "{f:?}");
}

#[test]
fn a_degree_before_the_high_school_diploma_is_a_violation() {
    let mut p = Profile::default();
    p.high_school.high_schools.push(crate::HighSchool {
        name: "Bridges Academy".into(),
        from: "2013-09".into(),
        to: "2017-06".into(),
        diploma: "Diploma".into(),
        diploma_date: "2017-06".into(),
        term_type: None,
    });
    let degree = |year| crate::Degree {
        kind: "Bachelor of Arts".into(),
        major: "Psychology".into(),
        second_major: None,
        minor: None,
        status: DegreeStatus::Awarded,
        month: "May".into(),
        year,
        note: None,
    };
    p.education.colleges.push(crate::College { name: "CONNECTICUT COLLEGE".into(), degrees: vec![degree(2016)] });
    let v: Vec<String> = validate(&p, TODAY).into_iter().filter(Finding::is_violation).map(|f| f.to_string()).collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("before the \"Bridges Academy\" diploma"));
    p.education.colleges[0].degrees[0].year = 2021;
    assert!(!validate(&p, TODAY).iter().any(Finding::is_violation));
}

#[test]
fn recommenders_need_consent_and_what_the_evaluation_form_requires() {
    let ok = Recommender {
        first_name: "Jane".into(),
        last_name: "Doe".into(),
        email: "jane@school.example".into(),
        due: "2026-10-25".into(),
        waive: true,
        message: "Thank you.".into(),
        programs: vec!["Yale University - GEPN".into()],
        kind: None,
    };
    let check = |consent: bool, rs: Vec<Recommender>| {
        let p = Profile { references: ReferencesFile { consent, recommenders: rs }, ..Default::default() };
        let mut out = Vec::new();
        references::check(&p, date!(2026 - 10 - 07), &mut out);
        out.iter().map(ToString::to_string).collect::<Vec<_>>()
    };
    assert_eq!(check(false, vec![]), Vec::<String>::new(), "no recommenders, no consent needed");
    assert_eq!(check(true, vec![ok.clone()]), Vec::<String>::new());
    assert!(check(false, vec![ok.clone()])[0].contains("consent = true is required"));
    let bad = Recommender {
        first_name: " ".into(),
        email: "jane at example".into(),
        due: "10/25/2026".into(),
        message: "x".repeat(references::MESSAGE_MAX + 1),
        programs: vec![],
        ..ok.clone()
    };
    let w = "VIOLATION  recommender \"jane at example\":";
    assert_eq!(
        check(true, vec![bad]),
        [
            format!("{w} first and last name are required"),
            format!("{w} not an email address"),
            format!("{w} message is 501 characters (max 500)"),
            format!("{w} due \"10/25/2026\" is not YYYY-MM-DD"),
            format!("{w} no programs"),
        ]
    );
    let long_email = format!("{}@school.example", "a".repeat(references::EMAIL_MAX));
    let too_long = format!("email is {} characters (max 100)", long_email.len());
    let past = Recommender {
        due: "2026-10-06".into(),
        message: " ".into(),
        email: long_email,
        programs: vec!["P".into(), "P".into()],
        ..ok.clone()
    };
    let out = check(true, vec![past, Recommender { email: "JANE@school.example".into(), ..ok.clone() }, ok]);
    for needle in [
        too_long.as_str(),
        "personal message is required",
        "due 2026-10-06 is past",
        "program \"P\" appears twice",
        "recommender \"jane@school.example\" appears twice",
    ] {
        assert!(out.iter().any(|m| m.contains(needle)), "{needle}: {out:?}");
    }
}
