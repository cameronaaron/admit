use super::*;
#[test]
fn malformed_units_and_missing_titles_cannot_hide_course_rows() {
    assert_eq!(course_line("BIO 1 LAVC Biology bad bad A 12", &COLLEGES), None);
    assert_eq!(
        course_line("BIO 1 LAVC 3.00 3.00 A 12", &COLLEGES),
        Some(("BIO 1".into(), "LAVC".into(), "".into(), "3.00".into(), "A".into()))
    );
}
