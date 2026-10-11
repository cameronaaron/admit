use super::*;

fn order(via: &'static str, id: &str, from: &str, to: &str, stage: Stage, ordered: &str) -> Order {
    Order {
        via,
        id: id.into(),
        from: from.into(),
        to: to.into(),
        status: format!("{stage:?}"),
        ordered: ordered.into(),
        changed: None,
        stage,
    }
}

fn send(via: &str, from: &str, to: &str, id: Option<&str>) -> TranscriptSend {
    TranscriptSend { from: from.into(), to: to.into(), via: via.into(), order: id.map(Into::into) }
}

fn file(sends: Vec<TranscriptSend>) -> TranscriptOrdersFile {
    TranscriptOrdersFile { email: "a@b.c".into(), sends }
}

#[test]
fn names_match_ignoring_case_and_punctuation_only() {
    assert!(same_school("AZUSA PACIFIC UNIVERSITY", "Azusa Pacific University"));
    assert!(same_school("California State University - Northridge", "California State University-Northridge"));
    assert!(!same_school("California State University", "California State University - Northridge"));
}

#[test]
fn a_sent_order_is_a_note_with_its_status_date() {
    let mut o = order(
        "clearinghouse",
        "97852158",
        "Connecticut College",
        "AZUSA PACIFIC UNIVERSITY",
        Stage::Sent,
        "2026-10-10",
    );
    o.status = "Electronic Transcript Sent".into();
    o.changed = Some("2026-10-10".into());
    let f = check(
        &file(vec![send("clearinghouse", "Connecticut College", "Azusa Pacific University", Some("97852158"))]),
        &[o],
    );
    assert_eq!(
        f,
        vec![Finding::Note(
            "transcript Connecticut College -> Azusa Pacific University: Electronic Transcript Sent on 2026-10-10 (clearinghouse 97852158)"
                .into()
        )]
    );
}

#[test]
fn an_open_order_is_pending_and_one_delivered_copy_outranks_it() {
    let s = send("parchment", "LACC", "NursingCAS", None);
    let open = order("parchment", "TR2", "LACC", "NursingCAS", Stage::Open, "2026-07-25");
    let f = check(&file(vec![s.clone()]), std::slice::from_ref(&open));
    assert_eq!(
        f,
        vec![Finding::Pending("transcript LACC -> NursingCAS: Open (ordered 2026-07-25) (parchment TR2)".into())]
    );
    let sent = order("parchment", "TR1", "LACC", "NursingCAS", Stage::Sent, "2026-07-24");
    let f = check(&file(vec![s]), &[open, sent]);
    assert!(matches!(&f[..], [Finding::Note(m)] if m.contains("TR1")), "{f:?}");
}

#[test]
fn a_held_order_is_a_violation_unless_another_copy_is_open() {
    let s = send("parchment", "LACC", "NursingCAS", None);
    let held = order("parchment", "TR1", "LACC", "NursingCAS", Stage::Held, "2026-07-24");
    let f = check(&file(vec![s.clone()]), std::slice::from_ref(&held));
    assert!(matches!(&f[..], [Finding::Violation(m)] if m.contains("held until someone acts")), "{f:?}");
    let open = order("parchment", "TR2", "LACC", "NursingCAS", Stage::Open, "2026-07-25");
    assert!(matches!(&check(&file(vec![s]), &[held, open])[..], [Finding::Pending(_)]));
}

#[test]
fn of_several_orders_in_one_stage_the_latest_is_shown() {
    let s = send("parchment", "LACC", "NursingCAS", None);
    let a = order("parchment", "TR1", "LACC", "NursingCAS", Stage::Open, "2026-07-24");
    let b = order("parchment", "TR2", "LACC", "NursingCAS", Stage::Open, "2026-08-01");
    for orders in [[a.clone(), b.clone()], [b, a]] {
        assert!(matches!(&check(&file(vec![s.clone()]), &orders)[..], [Finding::Pending(m)] if m.contains("TR2")));
    }
}

#[test]
fn a_send_without_an_order_says_what_is_missing() {
    let f = check(
        &file(vec![
            send("clearinghouse", "A", "B", None),
            send("parchment", "A", "B", None),
            send("parchment", "A", "B", Some("TR9")),
            send("fax", "A", "B", None),
        ]),
        &[],
    );
    assert_eq!(
        f,
        vec![
            Finding::Pending(
                "transcript A -> B: not ordered (or its Clearinghouse order number is not recorded: order = \"...\")"
                    .into()
            ),
            Finding::Pending("transcript A -> B: not ordered on parchment".into()),
            Finding::Violation(
                "transcript A -> B: parchment order TR9 not found with that sender and recipient".into()
            ),
            Finding::Violation(
                "transcript A -> B: via \"fax\" is not one of [\"clearinghouse\", \"parchment\"]".into()
            ),
        ]
    );
}

#[test]
fn an_order_must_match_service_sender_recipient_and_number() {
    let o = order("parchment", "TR1", "LACC", "NursingCAS", Stage::Sent, "2026-07-24");
    for s in [
        send("clearinghouse", "LACC", "NursingCAS", None),
        send("parchment", "ELAC", "NursingCAS", None),
        send("parchment", "LACC", "UCLA", None),
        send("parchment", "LACC", "NursingCAS", Some("TR2")),
    ] {
        let f = check(&file(vec![s]), std::slice::from_ref(&o));
        assert_eq!(f.len(), 2, "{f:?}");
        assert!(!matches!(f[0], Finding::Note(_)), "{f:?}");
    }
}

#[test]
fn an_order_no_send_accounts_for_is_noted_with_the_names_to_declare() {
    let mut o = order("parchment", "TR5G5BBF", "Los Angeles City College", "NursingCAS", Stage::Sent, "2026-07-24");
    o.status = "COMPLETE".into();
    assert_eq!(
        check(&file(vec![]), &[o]),
        vec![Finding::Note(
            "parchment order TR5G5BBF: Los Angeles City College -> NursingCAS (COMPLETE (ordered 2026-07-24)) is not in transcript_orders.toml"
                .into()
        )]
    );
}
