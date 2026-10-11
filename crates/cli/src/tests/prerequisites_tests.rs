use time::macros::date;

use super::*;

fn taken(college: &str, code: &str, year: &str, season: &str, grade: &str) -> TranscriptCourse {
    TranscriptCourse {
        year: year.into(),
        season: season.into(),
        code: code.into(),
        college: college.into(),
        title: String::new(),
        credits: "4.00".into(),
        grade: grade.into(),
        noncredit: false,
        term_months: None,
    }
}

fn req(name: &str, courses: &[(&str, &str)]) -> Requirement {
    Requirement {
        name: name.into(),
        courses: courses.iter().map(|(c, k)| CourseRef { college: (*c).into(), code: (*k).into() }).collect(),
        within_years: None,
        deferred: None,
    }
}

fn program(reqs: Vec<Requirement>) -> PrerequisiteProgram {
    PrerequisiteProgram {
        name: "UCLA MECN".into(),
        portal: Some("ucla".into()),
        start: Some("2027-09".into()),
        min_grade: Some("C".into()),
        source: None,
        requirements: reqs,
    }
}

fn transcripts(courses: Vec<TranscriptCourse>) -> Transcripts {
    Transcripts { courses, unreadable: BTreeMap::new() }
}

#[test]
fn the_rulings_on_the_captured_transcripts() {
    // UCLA's Sep 2026 rulings, as the transcripts print the courses.
    let t = transcripts(vec![
        taken("LAVC", "STAT 101", "2026", "Summer", "A"),
        taken("LACC", "FAM &CS 021", "2025", "Fall", "A"),
        taken("Connecticut College", "EAS 101", "2019", "Spring", "B+"),
        taken("Connecticut College", "EAS 106", "2018", "Spring", "B+"),
        taken("LAVC", "CHEM 051", "2026", "Fall", ""),
    ]);
    let mut nutrition = req("Nutrition", &[("LACC", "FAM & CS 21")]);
    nutrition.within_years = Some(5);
    let mut epi = req("Epidemiology", &[]);
    epi.deferred = Some("if admitted, may be completed at UCLA".into());
    let p = program(vec![
        req("Biostatistics", &[("LAVC", "STAT 101")]),
        nutrition,
        req("Written Communication", &[("Connecticut College", "EAS 101"), ("Connecticut College", "EAS 106")]),
        req("Chemistry", &[("LAVC", "CHEM 051")]),
        epi,
    ]);
    let f = check(&p, &t, date!(2026 - 10 - 06));
    assert!(matches!(&f[0], Finding::Note(m) if m.contains("met by LAVC STAT 101 (A, Summer 2026)")), "{f:?}");
    assert!(matches!(&f[1], Finding::Note(m) if m.contains("met by")), "leading zero and spacing differ: {f:?}");
    assert!(matches!(&f[2], Finding::Note(m) if m.contains("EAS 101") && m.contains("EAS 106")), "{f:?}");
    assert!(matches!(&f[3], Finding::Pending(m) if m.contains("in progress (Fall 2026, ends Dec 2026)")), "{f:?}");
    assert!(matches!(&f[4], Finding::Note(m) if m.contains("deferred")), "{f:?}");
    // January: the term is over and still ungraded.
    let f = check(&p, &t, date!(2027 - 01 - 10));
    assert!(matches!(&f[3], Finding::Pending(m) if m.contains("has no grade on the transcript yet")), "{f:?}");
}

#[test]
fn what_breaks_a_requirement() {
    let mut physiology = req("Physiology", &[("LAHC", "PHYSIOL 001")]);
    physiology.within_years = Some(5);
    let p = program(vec![
        physiology,
        req("Microbiology", &[("LACC", "MICRO 020")]),
        req("Anatomy", &[("LACC", "ANATOMY 001")]),
        req("Speech", &[("LAVC", "COMM C1000")]),
    ]);
    let t = transcripts(vec![
        taken("LAHC", "PHYSIOL 001", "2022", "Spring", "A"), // ends Jun 2022, before Sep 2022
        taken("LACC", "MICRO 020", "2026", "Spring", "D"),
        // No ANATOMY 001 at all.
        taken("LAVC", "COMM C1000", "2025", "Fall", "W"),
    ]);
    let f = check(&p, &t, date!(2026 - 10 - 06));
    assert!(f.iter().all(Finding::is_violation), "{f:?}");
    assert!(matches!(&f[0], Finding::Violation(m) if m.contains("older than 5 years before the Sep 2027 start")));
    assert!(matches!(&f[1], Finding::Violation(m) if m.contains("below the minimum")));
    assert!(matches!(&f[2], Finding::Violation(m) if m.contains("not on any transcript")));
    assert!(matches!(&f[3], Finding::Violation(m) if m.contains("graded W")));
}

#[test]
fn a_retake_that_counts_wins_and_an_unread_transcript_is_unchecked() {
    let p = program(vec![req("Stats", &[("Connecticut College", "MAT 210")]), req("X", &[("UMASS Amherst", "BIO 1")])]);
    let mut t = transcripts(vec![
        taken("Connecticut College", "MAT 210", "2019", "Fall", "W"),
        taken("Connecticut College", "MAT 210", "2020", "Spring", "B"),
    ]);
    t.unreadable.insert("UMASS Amherst".into(), "transcripts/conncoll.pdf: missing".into());
    let f = check(&p, &t, date!(2026 - 10 - 06));
    assert!(matches!(&f[0], Finding::Note(m) if m.contains("(B, Spring 2020)")), "{f:?}");
    assert!(matches!(&f[1], Finding::Pending(m) if m.contains("unchecked")), "absent in CI is not a failure: {f:?}");
}

#[test]
fn a_pass_grade_against_a_letter_minimum_is_asked_about() {
    let p = program(vec![req("Stats", &[("Connecticut College", "MAT 210")])]);
    let t = transcripts(vec![taken("Connecticut College", "MAT 210", "2020", "Spring", "S*")]);
    assert!(matches!(&check(&p, &t, date!(2026 - 10 - 06))[0], Finding::Pending(m) if m.contains("non-letter")));
}
