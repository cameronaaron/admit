use super::*;
#[test]
fn relay_state_and_report_links_handle_nonreports() {
    let page = "<form><input name='ICSID' value='s'></form><input id='ICStateNum' value='3'>";
    assert_eq!(ic_state(page), Some(IcState { sid: "s".into(), state_num: 3 }));
    assert_eq!(
        report_url("window.open(foo); window.open('help.html'); window.open('report.pdf')"),
        Some("report.pdf".into())
    );
    assert_eq!(report_url("window.open('help.html')"), None);
    assert_eq!(login_error("<span id='errorText'> </span>"), None);
}

#[test]
fn broken_markup_yields_nothing() {
    assert_eq!(login_error("<html></html>"), None, "no error element");
    assert_eq!(login_error("id=\"errorText\""), None, "no tag close");
    assert_eq!(login_error("<span id=\"errorText\">no close"), None, "no following tag");
    assert_eq!(ic_state("<form><input name='ICSID' value='sid'></form>"), None, "sid without a state number");
    assert_eq!(
        ic_state(
            "<form><input name='x' value='1'><input name='ICSID' value='s'></form><input id='ICStateNum' value='4'>"
        ),
        Some(IcState { sid: "s".into(), state_num: 4 })
    );
    assert_eq!(report_url("window.open("), None, "open call cut off");
    assert_eq!(report_url("window.open('report.pdf"), None, "quote never closes");
}
