//! Boundary cases for profile loading and validation.
use admit_domain::*;
use time::macros::date;

fn messages(p: &Profile) -> Vec<String> {
    validate::validate(p, date!(2026 - 10 - 06)).iter().map(ToString::to_string).collect()
}

#[test]
fn profile_roundtrips_all_sections_and_rejects_bad_files() {
    let dir = std::env::temp_dir().join(format!("admit-profile-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test directory");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profile");
    let p = Profile::load(&source).expect("profile");
    macro_rules! section {
        ($name:literal, $field:ident) => {
            std::fs::write(dir.join($name), toml::to_string(&p.$field).expect("serialize")).expect("write");
        };
    }
    section!("achievements.toml", achievements);
    section!("licenses.toml", licenses);
    section!("experiences.toml", experiences);
    section!("education.toml", education);
    section!("coursework.toml", coursework);
    section!("high_school.toml", high_school);
    section!("personal.toml", personal);
    section!("prerequisites.toml", prerequisites);
    section!("questions.toml", questions);
    section!("documents.toml", documents);
    section!("references.toml", references);
    section!("transcript_orders.toml", transcript_orders);
    assert_eq!(Profile::load(&dir).expect("reload"), p);
    std::fs::write(dir.join("achievements.toml"), "unknown = true").expect("write");
    let e = Profile::load(&dir).expect_err("unknown field");
    assert!(e.to_string().contains("achievements.toml"));
    assert!(matches!(e, LoadError::Toml { .. }));
    std::fs::remove_file(dir.join("achievements.toml")).expect("remove test file");
    std::fs::create_dir(dir.join("achievements.toml")).expect("directory in place of file");
    let e = Profile::load(&dir).expect_err("unreadable file");
    assert!(e.to_string().contains("achievements.toml"));
    assert!(matches!(e, LoadError::Io { .. }));
    std::fs::remove_dir_all(dir).expect("cleanup");
}

#[test]
fn each_later_section_file_fails_on_its_own() {
    let dir = std::env::temp_dir().join(format!("admit-profile-sections-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test directory");
    for name in [
        "licenses.toml",
        "experiences.toml",
        "education.toml",
        "coursework.toml",
        "high_school.toml",
        "personal.toml",
        "prerequisites.toml",
        "questions.toml",
        "documents.toml",
        "references.toml",
        "transcript_orders.toml",
    ] {
        let path = dir.join(name);
        std::fs::write(&path, "unknown = true").expect("write");
        let e = Profile::load(&dir).expect_err(name);
        assert!(e.to_string().contains(name), "{e}");
        assert!(matches!(e, LoadError::Toml { .. }));
        std::fs::remove_file(&path).expect("remove test file");
        std::fs::create_dir(&path).expect("directory in place of file");
        let e = Profile::load(&dir).expect_err(name);
        assert!(e.to_string().contains(name), "{e}");
        assert!(matches!(e, LoadError::Io { .. }));
        std::fs::remove_dir(&path).expect("remove test directory");
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn invalid_and_duplicate_records_report_the_actual_fields() {
    let mut p = Profile::default();
    let a = Achievement {
        kind: AchievementKind::Award,
        name: " ".into(),
        organization: Some("é".repeat(61)),
        date: Some("bad".into()),
        state: "California".into(),
        country: "Canada".into(),
        description: Some("the the award".into()),
        ids: [("test".into(), 4)].into_iter().collect(),
    };
    p.achievements.achievements = vec![
        a.clone(),
        a,
        Achievement {
            kind: AchievementKind::Honor,
            name: "Quiet".into(),
            organization: None,
            date: None,
            state: "California".into(),
            country: "United States".into(),
            description: None,
            ids: Default::default(),
        },
    ];
    p.licenses.licenses.push(License {
        kind: LicenseKind::Licensure,
        title: "License".into(),
        organization: "Board".into(),
        description: Some("Valid.. now".into()),
        valid_until: Some("bad".into()),
        ids: Default::default(),
    });
    p.education.colleges.push(College {
        name: "School".into(),
        degrees: vec![Degree {
            kind: "BA".into(),
            major: "History".into(),
            second_major: Some("Art".into()),
            minor: None,
            status: DegreeStatus::Awarded,
            month: "Smarch".into(),
            year: 2020,
            note: None,
        }],
    });
    p.high_school.high_schools.push(HighSchool {
        name: "HS".into(),
        from: "2020-01".into(),
        to: "2019-01".into(),
        diploma: "Diploma".into(),
        diploma_date: "bad".into(),
        term_type: None,
    });
    let f = messages(&p);
    for expected in [
        "no name",
        "organization is 61 characters",
        "date \"bad\"",
        "appears twice",
        "valid_until",
        "Smarch",
        "before from",
        "diploma_date",
        "repeated word",
        "doubled period",
    ] {
        assert!(f.iter().any(|m| m.contains(expected)), "missing {expected}: {f:?}");
    }
    let serialized = toml::to_string(&p.achievements).expect("serialize");
    assert!(serialized.contains("Canada") && serialized.contains("test"));
}

#[test]
fn experience_date_errors_and_hours_are_not_silently_skipped() {
    let base = Experience {
        kind: ExperienceKind::PatientCare,
        organization: "Clinic".into(),
        title: "Care".into(),
        state: "California".into(),
        country: "United States".into(),
        start: "2020-01-01".into(),
        end: Some("2020-02-01".into()),
        current: false,
        status: "Part time".into(),
        hours_per_week: 10,
        weeks: 4,
        total_hours: 40,
        compensated: true,
        volunteer: false,
        description: "Care".into(),
        supervisor: Supervisor { first_name: "Test".into(), ..Default::default() },
        ids: Default::default(),
    };
    for (start, end, current, hours, expected) in [
        ("bad", Some("2020-02-01"), false, 10, "start"),
        ("2020-01-01", Some("bad"), false, 10, "end"),
        ("2020-01-01", None, false, 10, "no end date"),
        ("2020-01-01", Some("2027-01-01"), false, 10, "not marked current"),
        ("2020-01-01", Some("2019-01-01"), false, 10, "ends before"),
        ("2020-01-01", Some("2020-02-02"), false, 169, "169 hours per week"),
        ("2026-10-01", None, true, 10, "total_hours"),
    ] {
        let p = Profile {
            experiences: ExperiencesFile {
                experiences: vec![Experience {
                    start: start.into(),
                    end: end.map(Into::into),
                    current,
                    hours_per_week: hours,
                    total_hours: 1,
                    ..base.clone()
                }],
            },
            ..Default::default()
        };
        let f = messages(&p);
        assert!(f.iter().any(|m| m.contains(expected)), "{expected}: {f:?}");
    }
    assert!(!base.supervisor.is_empty());
    assert!(toml::to_string(&ExperiencesFile { experiences: vec![base] }).expect("serialize").contains("first_name"));
}

#[test]
fn employment_with_consistent_hours_is_pending_and_not_a_math_note() {
    let p = Profile {
        experiences: ExperiencesFile {
            experiences: vec![Experience {
                kind: ExperienceKind::Employment,
                organization: "Clinic".into(),
                title: "Aide".into(),
                state: "California".into(),
                country: "United States".into(),
                start: "2020-01-01".into(),
                end: Some("2020-02-01".into()),
                current: false,
                status: "Part time".into(),
                hours_per_week: 10,
                weeks: 4,
                total_hours: 40,
                compensated: true,
                volunteer: false,
                description: "Aide".into(),
                supervisor: Supervisor::default(),
                ids: Default::default(),
            }],
        },
        ..Default::default()
    };
    let findings = validate::validate(&p, date!(2026 - 10 - 06));
    assert!(findings.iter().all(|f| !f.is_violation()), "{findings:?}");
    let text = findings.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert!(text.contains("PENDING") && text.contains("patient-care"), "{text}");
    assert!(!text.contains("total_hours"), "{text}");
}

#[test]
fn degree_months_school_diplomas_and_license_expiry_validate_boundaries() {
    let mut p = Profile::default();
    p.education.colleges.push(College {
        name: "School".into(),
        degrees: vec![Degree {
            kind: "BA".into(),
            major: "Art".into(),
            second_major: None,
            minor: None,
            status: DegreeStatus::Awarded,
            month: "NotAMonth".into(),
            year: 2020,
            note: None,
        }],
    });
    p.high_school.high_schools.push(HighSchool {
        name: "HS".into(),
        from: "bad".into(),
        to: "2020-01".into(),
        diploma: "Diploma".into(),
        diploma_date: "2027-01".into(),
        term_type: None,
    });
    p.licenses.licenses.push(License {
        kind: LicenseKind::Certification,
        title: "BLS".into(),
        organization: "Board".into(),
        description: None,
        valid_until: None,
        ids: Default::default(),
    });
    let f = messages(&p);
    assert!(f.iter().any(|m| m.contains("month \"NotAMonth\" is not a month name")));
    assert!(f.iter().any(|m| m.contains("diploma_date is in the future")));
    assert!(f.iter().all(|m| !m.contains("BLS")));
}

#[test]
fn a_finding_message_drops_only_the_severity() {
    use admit_domain::Finding;
    for f in [Finding::Violation("a: b".into()), Finding::Pending("a: b".into()), Finding::Note("a: b".into())] {
        assert_eq!(f.message(), "a: b");
        assert!(f.to_string().ends_with(f.message()));
    }
}
