//! Elmbridge and Connecticut College transcripts: real layouts, synthetic grades.

use admit_sources::{conncoll, elmbridge};

const ELM: &str = include_str!("fixtures/elmbridge_layout.txt");
const CONN: &str = include_str!("fixtures/conncoll_raw.txt");
const TRANSFERS: [&str; 3] = ["Trinity Christian College", "Calif St Univ Northridg", "UMASS Amherst"];

#[test]
fn elmbridge_parses_all_eleven_and_reconciles_with_credits_earned() {
    let c = elmbridge::parse(ELM).expect("reconciles with Credits Earned: 30.00");
    assert_eq!(c.len(), 11);
    let res = c.iter().find(|x| x.code == "RES 621A").cloned();
    assert_eq!(
        res.map(|x| (x.title, x.season, x.year, x.credits, x.grade)),
        Some((
            "Methods and Techniques of Educational Research Part 1".into(),
            "Spring".into(),
            "2025".into(),
            "1.00".into(),
            "B+".into()
        ))
    );
    assert!(c.iter().any(|x| x.code == "CAP 622" && x.grade == "P"));
}

#[test]
fn elmbridge_dropping_a_row_fails_reconciliation() {
    let broken = ELM.replacen("IEC 621", "IEC-621", 1);
    assert!(matches!(elmbridge::parse(&broken), Err(admit_sources::ParseError::Unreconciled(_))));
}

#[test]
fn conncoll_parses_home_and_transfer_rows_and_reconciles() {
    let c = conncoll::parse(CONN, &TRANSFERS).expect("reconciles with OVERALL 146.00 attempted (S* excluded)");
    assert_eq!(c.len(), 46);
    let get = |code: &str| c.iter().filter(|x| x.code == code).cloned().collect::<Vec<_>>();
    // The semester carries forward from a term's first row.
    assert_eq!(get("COM 110L")[0].season, "Fall");
    assert_eq!(get("COM 110L")[0].credits, "0.00");
    // Transfer rows keep their own institution, then the home college resumes.
    assert_eq!(get("BUS 201")[0].college, "Trinity Christian College");
    assert_eq!(get("PSYC 350")[0].college, "UMASS Amherst");
    assert_eq!(get("AT 402")[0].college, conncoll::INSTITUTION);
    // The code on the transcript (the portal had PSY 323 at one point).
    assert_eq!(get("PSY 321")[0].title, "SPORT & EXERCISE PSYCHOLOGY");
    assert_eq!(get("MAT 210").iter().map(|x| x.grade.as_str()).collect::<Vec<_>>(), vec!["W", "S*"]);
}

#[test]
fn missing_totals_and_damaged_rows_are_rejected() {
    use admit_sources::{ParseError, conncoll, elmbridge, laccd};
    for e in [conncoll::parse("", &[]), elmbridge::parse(""), laccd::parse("", &laccd::COLLEGES)] {
        assert!(matches!(e, Err(ParseError::Empty)));
    }
    let conn = include_str!("fixtures/conncoll_raw.txt");
    let no_totals = conn.lines().filter(|l| !l.starts_with("OVERALL")).collect::<Vec<_>>().join("\n");
    assert!(conncoll::parse(&no_totals, &[]).expect_err("missing totals").to_string().contains("no OVERALL"));
    let wrong = conn.replace("OVERALL ", "OVERALL 999.00 ");
    assert!(conncoll::parse(&wrong, &[]).is_err());
    let orphan = "3.00 COM 101 Orphan A\n3.00 COM 102 Orphan ABSENT\n";
    assert!(matches!(conncoll::parse(orphan, &[]), Err(ParseError::Empty)));
    let elm = include_str!("fixtures/elmbridge_layout.txt");
    let no_totals = elm.lines().filter(|l| !l.contains("Earned:")).collect::<Vec<_>>().join("\n");
    assert!(elmbridge::parse(&no_totals).expect_err("missing totals").to_string().contains("Credits Earned"));
}

#[test]
fn in_progress_and_leading_dot_credits_are_kept() {
    // "XX" reaches `t == "IP"` and fails it; "IP" is the grade that arm accepts.
    // ".50" is the credits form that is printed without a leading zero.
    let text = "\
3.00 COM 101 SKIPPED Fall 2017 XX
.50 COM 110 LAB Fall 2017 IP
OVERALL 0.50
";
    let c = conncoll::parse(text, &[]).expect("reconciles");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].code, "COM 110");
    assert_eq!(c[0].credits, "0.50");
    assert_eq!(c[0].grade, "IP");
    assert_eq!(c[0].season, "Fall");
    assert_eq!(c[0].year, "2017");
}
