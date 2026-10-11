use time::macros::date;

use super::*;

const TODAY: Date = date!(2026 - 10 - 07);

fn dl(label: &str, date: Date) -> Deadline {
    Deadline { label: label.into(), date, shown: date.to_string() }
}

fn p(m: &str) -> Finding {
    Finding::Pending(m.into())
}

/// The shapes the live 2026-10-07 audit produced for NursingCAS.
fn nursingcas() -> Readiness {
    let davis = dl("UC Davis - MEPN", date!(2026 - 11 - 01));
    let yale = dl("Yale - GEPN", date!(2027 - 01 - 02));
    let soon = grade(&davis, TODAY);
    assert!(matches!(soon, Finding::Pending(_)), "25 days out is Pending: {soon:?}");
    assert_eq!(soon.message(), "UC Davis - MEPN: deadline 2026-11-01 is in 25 day(s)");
    assess(
        "nursingcas",
        false,
        vec![yale, davis],
        vec![
            soon,
            p("UC Davis - MEPN [INCOMPLETE]: Questions, References"),
            p("UC Davis - MEPN: 0 of 3 recommendation(s) requested"),
            p("UC Davis - MEPN Dual: 0 of 2 recommendation(s) requested"),
            p("9 program(s) not paid (fee waiver Requested)"),
            Finding::Note("Yale - GEPN: deadline 2027-01-02 (87 days)".into()),
        ],
        TODAY,
    )
}

#[test]
fn findings_go_to_the_program_they_name_and_the_rest_block_every_program() {
    let r = nursingcas();
    let labels: Vec<&str> = r.programs.iter().map(|p| p.deadline.label.as_str()).collect();
    assert_eq!(labels, ["UC Davis - MEPN", "Yale - GEPN"], "soonest first");
    assert_eq!(
        r.programs[0].blockers,
        vec![
            p("UC Davis - MEPN [INCOMPLETE]: Questions, References"),
            p("UC Davis - MEPN: 0 of 3 recommendation(s) requested"),
        ]
    );
    assert!(r.programs[1].blockers.is_empty(), "notes are not blockers");
    // A label that merely starts with another program's is not that program.
    assert_eq!(
        r.wide,
        vec![
            p("UC Davis - MEPN Dual: 0 of 2 recommendation(s) requested"),
            p("9 program(s) not paid (fee waiver Requested)"),
        ]
    );
    assert!(!r.program_ready(&r.programs[1]), "portal-wide blockers block Yale too");
    assert!(!r.ready());
}

#[test]
fn an_upcoming_deadline_is_not_a_blocker_but_a_passed_one_is() {
    let d = dl("A - B", date!(2026 - 10 - 20));
    let r = assess("x", false, vec![d.clone()], vec![grade(&d, TODAY)], TODAY);
    assert!(r.ready(), "{r:?}");
    let gone = dl("A - B", date!(2026 - 10 - 01));
    let r = assess("x", false, vec![gone.clone()], vec![grade(&gone, TODAY)], TODAY);
    assert!(!r.ready());
    assert!(r.programs[0].blockers[0].is_violation());
}

#[test]
fn ready_reports_each_program_and_fails_unless_all_are_ready() {
    let done = assess("ucla", false, vec![dl("ucla application", date!(2026 - 11 - 01))], vec![], TODAY);
    let sent = assess("apu", true, vec![], vec![p("apu: Essay awaited")], TODAY);
    let (text, ok) = render_ready(TODAY, &[], &[done.clone(), sent.clone()]);
    assert!(ok, "{text}");
    assert!(text.contains("ucla: READY, 1 of 1 program(s) ready"), "{text}");
    assert!(text.contains("ucla application (due 2026-11-01, in 25 day(s)): READY"), "{text}");
    assert!(text.contains("apu: SUBMITTED, 1 item(s) awaited\n  PENDING    apu: Essay awaited"), "{text}");

    let (text, ok) = render_ready(TODAY, &[], &[nursingcas()]);
    assert!(!ok);
    assert!(text.contains("nursingcas: NOT READY, 0 of 2 program(s) ready"), "{text}");
    assert!(text.contains("every program: 2 blocking"), "{text}");
    assert!(text.contains("UC Davis - MEPN (due 2026-11-01, in 25 day(s)): 2 blocking"), "{text}");

    let (text, ok) = render_ready(TODAY, &[Finding::Violation("hours".into()), p("x")], std::slice::from_ref(&done));
    assert!(!ok, "a profile violation blocks every application");
    assert!(text.contains("profile: 1 violation(s)"), "{text}");

    let broken = Readiness { portal: "ucla".into(), error: Some("HTTP 503".into()), ..Default::default() };
    let (text, ok) = render_ready(TODAY, &[], &[broken]);
    assert!(!ok && text.contains("ucla: UNCHECKED: HTTP 503"), "{text}");
}

#[test]
fn remind_names_only_unready_programs_inside_the_window() {
    let all = [nursingcas()];
    let text = render_remind(TODAY, 30, &all);
    assert!(text.starts_with("2026-11-01 (in 25 day(s)) nursingcas: UC Davis - MEPN - NOT READY, 2 blocking here, 2 for every nursingcas program\n"), "{text}");
    assert!(!text.contains("Yale"), "Yale is 87 days out:\n{text}");
    assert_eq!(text.lines().count(), 5, "{text}");
    assert!(render_remind(TODAY, 20, &all).is_empty(), "nothing due within 20 days");

    let ucla = assess("ucla", false, vec![dl("ucla application", date!(2026 - 11 - 01))], vec![p("x")], TODAY);
    let early = assess("apu", false, vec![dl("apu application", date!(2026 - 10 - 15))], vec![p("y")], TODAY);
    let text = render_remind(TODAY, 30, &[ucla, nursingcas(), early]);
    let firsts: Vec<&str> =
        text.lines().filter(|l| !l.starts_with(' ')).map(|l| &l[..l.find(':').unwrap_or(0)]).collect();
    assert_eq!(
        firsts,
        ["2026-10-15 (in 8 day(s)) apu", "2026-11-01 (in 25 day(s)) nursingcas", "2026-11-01 (in 25 day(s)) ucla"],
        "soonest first, then by portal:\n{text}"
    );

    let ready = assess("ucla", false, vec![dl("ucla application", date!(2026 - 10 - 10))], vec![], TODAY);
    assert!(render_remind(TODAY, 30, &[ready]).is_empty(), "a ready program needs no reminder");

    let passed = assess("ucla", false, vec![dl("ucla application", date!(2026 - 10 - 01))], vec![p("x")], TODAY);
    assert!(render_remind(TODAY, 0, &[passed]).contains("PASSED 6 day(s) ago"));

    let sent = assess("apu", true, vec![dl("apu", date!(2026 - 10 - 08))], vec![p("x")], TODAY);
    assert!(render_remind(TODAY, 30, &[sent]).is_empty(), "submitted applications are done");

    let broken = Readiness { portal: "ucla".into(), error: Some("HTTP 503".into()), ..Default::default() };
    assert_eq!(render_remind(TODAY, 30, &[broken]), "ucla: UNCHECKED, its deadlines are unknown: HTTP 503\n");
}
