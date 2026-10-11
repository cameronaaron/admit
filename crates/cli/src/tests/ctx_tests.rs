use time::macros::{date, datetime, offset};

use super::*;

#[test]
fn an_evening_in_california_is_still_today() {
    // 22:28 PDT on 2026-10-06 is 05:28 UTC on the 7th.
    assert_eq!(date_at(datetime!(2026-10-07 05:28 UTC), offset!(-7)), date!(2026 - 10 - 06));
    assert_eq!(date_at(datetime!(2026-10-07 05:28 UTC), offset!(UTC)), date!(2026 - 10 - 07));
}
