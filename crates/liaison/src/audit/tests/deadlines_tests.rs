use serde_json::json;
use time::macros::date;

use super::*;

#[test]
fn display_dates_parse() {
    assert_eq!(display_date("Jan 15, 2027, 11:59 PM EST"), Some(date!(2027 - 01 - 15)));
    assert_eq!(display_date("Nov 1, 2026, 11:59 PM EST"), Some(date!(2026 - 11 - 01)));
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    for (i, name) in months.iter().enumerate() {
        let got = display_date(&format!("{name} 2, 2024")).expect(name);
        let month = time::Month::try_from((i as u8) + 1).expect("month");
        assert_eq!(got, time::Date::from_calendar_date(2024, month, 2).expect("date"));
    }
    for bad in ["", "J 1, 2024", "Xyz 1, 2024", "Jan", "Jan xx, 2024", "Jan 2", "Jan 2, year", "Jan 32, 2024"] {
        assert_eq!(display_date(bad), None, "{bad}");
    }
}

#[test]
fn the_earlier_date_wins_and_windows_grade_the_finding() {
    let progs = json!([
        // Captured disagreement: display Jan 15, field Mar 16.
        {"organizationName": "Johns Hopkins University", "programName": "MSN Entry into Nursing",
         "deadline": "2027-03-16", "deadlineDisplay": "Jan 15, 2027, 11:59 PM EST", "submissionStatus": "In Progress"},
        {"organizationName": "UCSB", "programName": "ABSN", "deadline": "2026-11-16",
         "deadlineDisplay": "Nov 15, 2026, 11:59 PM EST", "submissionStatus": "In Progress"},
        {"organizationName": "Old", "programName": "Gone", "deadline": "2026-09-01",
         "deadlineDisplay": "", "submissionStatus": "In Progress"},
        {"organizationName": "Done", "programName": "Sent", "deadline": "2026-09-01",
         "deadlineDisplay": "", "submissionStatus": "Submitted"},
        {"organizationName": "Bad", "programName": "Dates", "deadline": "not-a-date",
         "deadlineDisplay": "Not a date", "submissionStatus": "In Progress"}]);
    let f = check(&progs, date!(2026 - 10 - 20));
    assert_eq!(f.len(), 3, "submitted programs are skipped: {f:?}");
    assert!(matches!(&f[0], Finding::Note(m) if m.contains("Jan 15, 2027") && m.contains("(87 days)")), "{f:?}");
    assert!(matches!(&f[1], Finding::Pending(m) if m.contains("in 26 day(s)")), "{f:?}");
    assert!(matches!(&f[2], Finding::Violation(m) if m.contains("has passed")), "{f:?}");
}

#[test]
fn deadlines_by_selected_program_skip_submitted_and_undated_ones() {
    // `programSelectedId` is what /getProgramNavigation and the evaluations
    // endpoint key programs by (captured 2026-10-06: 79437723 in both).
    let progs = json!([
        {"programSelectedId": 79437723, "deadline": "2027-03-16", "deadlineDisplay": "Jan 15, 2027, 11:59 PM EST",
         "submissionStatus": "In Progress"},
        {"programSelectedId": 2, "deadline": "2026-09-01", "submissionStatus": "Submitted"},
        {"programSelectedId": 3, "deadline": "never", "submissionStatus": "In Progress"}]);
    assert_eq!(by_program(&progs), BTreeMap::from([(79437723, date!(2027 - 01 - 15))]));
}
