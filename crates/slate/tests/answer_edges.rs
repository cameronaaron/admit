//! Branches of reading and encoding an answer that the captured pages never take.

use std::collections::BTreeMap;

use admit_slate::answer::{self, Answer};
use admit_slate::page::{self, Page};

fn one(s: &str) -> Answer {
    Answer::One(s.into())
}

fn page_of(html: &str) -> Page {
    page::page("frm?x", html)
}

fn q(html: &str) -> page::Question {
    page_of(html).questions.into_iter().next().expect("question")
}

#[test]
fn missing_controls_and_unusable_answers_are_refused() {
    let text = q("<div class=\"form_question\" data-id=\"t\" data-type=\"text\" data-export=\"t\"></div>");
    assert!(answer::current(&text).is_none());

    let select = q("<div class=\"form_question\" data-id=\"s\" data-type=\"select\" data-export=\"s\"></div>");
    assert!(answer::current(&select).is_none());
    assert!(answer::encode(&select, &one("A")).expect_err("control").contains("no control"));
    let select = q(
        "<div class=\"form_question\" data-id=\"s\" data-type=\"select\"><select name=\"form_s\"><option value=\"A\">A</option></select></div>",
    );
    assert!(answer::encode(&select, &Answer::Many(vec!["A".into()])).expect_err("list").contains("not a list"));

    let checks = q("<div class=\"form_question\" data-id=\"c\" data-type=\"checkbox\"></div>");
    assert!(answer::current(&checks).is_none());
    assert!(answer::encode(&checks, &one("A")).expect_err("control").contains("no control"));
    let checks = q(
        "<div class=\"form_question\" data-id=\"c\" data-type=\"checkbox\"><input type=\"checkbox\" name=\"form_c\" value=\"1\" data-text=\"Yes\"></div>",
    );
    assert!(answer::encode(&checks, &Answer::Many(vec!["No".into()])).expect_err("choice").contains("not a choice"));

    let date = q("<div class=\"form_question\" data-id=\"d\" data-type=\"date\"></div>");
    assert!(answer::encode(&date, &Answer::Many(vec!["2000-01-01".into()])).expect_err("list").contains("not a list"));
    let date = q(
        "<div class=\"form_question\" data-id=\"d\" data-type=\"date\"><select name=\"form_d_y\"><option value=\"2000\" selected>2000</option></select><select name=\"form_d_d\"><option value=\"01\" selected>1</option></select></div>",
    );
    assert!(answer::encode(&date, &one("2000-01-01")).expect_err("part").contains("date control missing"));

    assert!(answer::current(&q(
        "<div class=\"form_question\" data-id=\"d\" data-type=\"date\"><select name=\"form_d_m\"><option value=\"01\" selected=\"selected\">Jan</option></select><select name=\"form_d_d\"><option value=\"02\" selected=\"selected\">2</option></select></div>"
    )).is_none(), "no year");
    assert!(answer::current(&q(
        "<div class=\"form_question\" data-id=\"d\" data-type=\"date\"><select name=\"form_d_y\"><option value=\"2000\" selected=\"selected\">2000</option></select><select name=\"form_d_d\"><option value=\"02\" selected=\"selected\">2</option></select></div>"
    )).is_none(), "no month");
    assert!(answer::current(&q(
        "<div class=\"form_question\" data-id=\"d\" data-type=\"date\"><select name=\"form_d_y\"><option value=\"2000\" selected=\"selected\">2000</option></select><select name=\"form_d_m\"><option value=\"01\" selected=\"selected\">Jan</option></select><select name=\"form_d_d\"><option value=\"\"></option></select></div>"
    )).is_none(), "empty day");
    assert!(answer::current(&q(
        "<div class=\"form_question\" data-id=\"ym\" data-type=\"dateym\"><select name=\"form_ym_m\"><option value=\"08\" selected=\"selected\">Aug</option></select></div>"
    )).is_none(), "no year on a month-year date");
    assert!(answer::current(&q(
        "<div class=\"form_question\" data-id=\"ym\" data-type=\"dateym\"><select name=\"form_ym_y\"><option value=\"2020\" selected=\"selected\">2020</option></select></div>"
    )).is_none(), "no month on a month-year date");
}

#[test]
fn a_long_choice_list_names_only_the_first_dozen() {
    let options: String = (0..13).map(|i| format!("<option value=\"{i}\">{i}</option>")).collect();
    let question = q(&format!(
        "<div class=\"form_question\" data-id=\"s\" data-type=\"select\"><select name=\"form_s\">{options}</select></div>"
    ));
    let err = answer::encode(&question, &one("nope")).expect_err("choice");
    assert!(err.contains(" | …"), "{err}");
}

#[test]
fn a_formula_that_does_not_parse_stops_the_page_post() {
    let p = page_of(
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"a\" data-formula=\"1 +\"></div>",
    );
    assert!(answer::page_post(&p, &[]).expect_err("formula").contains("does not parse"));
    for (formula, needle) in [
        ("1..2", "bad number"),
        ("@nope", "no question"),
        ("@ghost_text", "no question"),
        ("1 a", "unsupported"),
        ("1 2", "does not parse"),
        ("@missing + 1", "no question"),
    ] {
        let bad = page_of(&format!(
            "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"1\" data-formula=\"{formula}\"></div>"
        ));
        assert!(answer::page_post(&bad, &[]).expect_err(formula).contains(needle), "{formula}");
    }
    let half = page_of(
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"1\" data-formula=\"3/2\"></div>",
    );
    let posted = answer::page_post(&half, &[]).expect("fraction");
    assert!(posted.fields.iter().any(|(_, v)| v == "1.5"), "{posted:?}");
    let bare = page_of(
        "<div class=\"form_question\" data-id=\"c\" data-type=\"select\" data-export=\"sys:color\"></div><div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"1\" data-formula=\"@sys:color_text\"></div>",
    );
    assert!(answer::page_post(&bare, &[]).expect_err("text").contains("no question"));
    let want = BTreeMap::from([("q".to_string(), one("b"))]).into();
    let plan = answer::plan(std::slice::from_ref(&p), &want);
    assert_eq!(plan.changes.len(), 1);
}

#[test]
fn an_answer_that_moved_to_another_value_is_named() {
    let before = page_of(
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"old\"></div>",
    );
    let after = page_of(
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\" data-export=\"q\"><input name=\"form_q\" value=\"new\"></div>",
    );
    let moved = admit_slate::collateral(Some(&before), &after, &Default::default());
    assert_eq!(moved, vec!["q \"old\" -> \"new\"".to_string()]);
}
