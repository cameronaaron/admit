use super::*;

#[test]
fn ic_state_takes_the_latest_number() {
    let page = "<input type='hidden' name='ICSID' id='ICSID' value='S1' /><input id='ICStateNum' value='5'/>\
                <script>ICStateNum.value=7;</script>";
    assert_eq!(ic_state(page), Some(IcState { sid: "S1".into(), state_num: 7 }));
    assert_eq!(ic_state("ICStateNum.value=3;"), None, "no ICSID, no state");
}
