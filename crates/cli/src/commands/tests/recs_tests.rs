use time::macros::date;

use super::*;

fn row(label: &str, need: i64, requested: i64, received: i64, deadline: Option<Date>) -> Recs {
    Recs { label: label.into(), need, requested, received, deadline }
}

#[test]
fn rows_sort_by_deadline_and_count_letters_still_to_ask_for() {
    let today = date!(2026 - 10 - 07);
    let out = render(
        today,
        vec![
            row("Yale - GEPN", 3, 1, 0, Some(date!(2027 - 01 - 02))),
            row("Sent - Already", 2, 0, 0, None),
            row("UC Davis - MEPN", 3, 3, 1, Some(date!(2026 - 11 - 01))),
            row("Rush - GEM", 2, 2, 2, Some(date!(2027 - 01 - 02))),
        ],
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[1], "  WAITING  3/1 of 3  2026-11-01 (25 days)   UC Davis - MEPN");
    assert_eq!(lines[2], "  DONE     2/2 of 2  2027-01-02 (87 days)   Rush - GEM");
    assert_eq!(lines[3], "  ASK      1/0 of 3  2027-01-02 (87 days)   Yale - GEPN");
    assert_eq!(lines[4], "  ASK      0/0 of 2  submitted              Sent - Already");
    assert_eq!(
        lines[5], "2 letter(s) still to request across unsubmitted programs",
        "a submitted program's are not counted"
    );
}
