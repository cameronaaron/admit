use super::*;

#[test]
fn course_line_shapes() {
    let c = |l| course_line(l, &COLLEGES);
    assert_eq!(
        c("CH DEV 001 LAMC Child Growth&Develop 3.00 3.00 A 12 CSU/UC"),
        Some(("CH DEV 001".into(), "LAMC".into(), "Child Growth&Develop".into(), "3.00".into(), "A".into()))
    );
    // A title ending in a lone capital letter, then a two-letter grade.
    assert_eq!(
        c("VOC ED 912CE LAVC Adobe Premiere Video Editing W 0.00 0.00 NP 0 NDA").map(|x| (x.2, x.4)),
        Some(("Adobe Premiere Video Editing W".into(), "NP".into()))
    );
    // In progress: no grade column.
    assert_eq!(c("KIN 246 LACC Body Sculpting 1.00 0.00 0 CSU/UC").map(|x| x.4), Some(String::new()));
    assert_eq!(c("TUTOR 001T LACC Supervised Tutoring 0.00 0.00 * 0 NDA").map(|x| x.4), Some("*".into()));
    assert_eq!(c("Term GPA 4.00 Term Totals 18.00 18.00 18.00 72"), None);
    assert_eq!(c("Cum GPA w/o NDA 3.69 41.00 36.00 129"), None);
}

#[test]
fn a_dropped_line_fails_reconciliation() {
    let text = "2026 Spring (Feb 9, 2026 - Jun 8, 2026)\n\
                ANATOMY 001 LACC Intro To Human Anat 4.00 4.00 A 16 CSU/UC\n\
                MICRO 020 LACC General Microbiology 4.00 4.00 A 16 CSU/UC\n\
                Term GPA 4.00 Term Totals 12.00 12.00 12.00 48\n";
    assert!(matches!(parse(text, &COLLEGES), Err(ParseError::Unreconciled(_))));
    let ok = text.replace("12.00 12.00 12.00 48", "8.00 8.00 8.00 32");
    assert_eq!(parse(&ok, &COLLEGES).map(|c| c.len()).ok(), Some(2));
}

#[test]
fn stated_totals_with_no_parsed_rows_name_the_gap() {
    let text = "\
2026 Spring (Feb 9, 2026 - Jun 8, 2026)
BIO 1 LAVC Biology 3.00 3.00 A 12 CSU
Term GPA 4.00 Term Totals 3.00 3.00 3.00 12
Beginning of Non Credit Record
2026 Fall (Sep 1, 2026 - Dec 21, 2026)
Term GPA 0.00 Term Totals 5.00 0.00 0.00 0
";
    let err = parse(text, &COLLEGES).expect_err("noncredit totals");
    let ParseError::Unreconciled(lines) = err else { panic!("unreconciled") };
    assert!(lines.iter().any(|l| l.contains("noncredit") && l.contains("5.00") && l.contains("0.00")), "{lines:?}");
}

#[test]
fn prose_between_courses_is_not_a_row() {
    let text = "\
2026 Spring (Feb 9, 2026 - Jun 8, 2026)
not a course
BIO 1 LAVC Biology 3.00 3.00 A 12 CSU
Term GPA 4.00 Term Totals 3.00 3.00 3.00 12
";
    let courses = parse(text, &COLLEGES).expect("reconciles");
    assert_eq!(courses.len(), 1);
    assert_eq!(course_line("too short", &COLLEGES), None);
    assert_eq!(course_line("", &COLLEGES), None);
}
