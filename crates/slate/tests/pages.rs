//! The page reader and planner against markup captured from
//! apply.grad.ucla.edu on 2026-10-06 (personal values replaced).

use std::collections::{BTreeMap, BTreeSet};

use admit_domain::Finding;
use admit_slate::answer::{self, Answer, AnswersFile, current};
use admit_slate::page::{self, Kind};

const PAGE: &str = include_str!("fixtures/page.html");
const REVIEW: &str = include_str!("fixtures/review.html");
const HREF: &str = "frm?7946b795-1470-4fc2-bc93-35f853dc8846";

fn pages() -> Vec<page::Page> {
    vec![page::page(HREF, PAGE)]
}

fn q<'a>(p: &'a page::Page, export: &str) -> &'a page::Question {
    p.questions.iter().find(|q| q.export.as_deref() == Some(export) && !q.readonly).expect(export)
}

fn answers(pairs: &[(&str, Answer)]) -> BTreeMap<String, Answer> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn one(s: &str) -> Answer {
    Answer::One(s.into())
}

#[test]
fn menu_lists_every_section_but_home() {
    let nav = page::nav(PAGE);
    assert_eq!(nav.first().map(|n| n.title.as_str()), Some("Frequently Asked Questions"));
    assert_eq!(nav.last().map(|n| n.href.as_str()), Some("review"));
    assert_eq!(nav.len(), 24);
    let lists: Vec<&str> = nav.iter().filter(|n| !n.is_form()).map(|n| n.href.as_str()).collect();
    assert_eq!(lists, ["aca", "job", "tes", "certify", "review"]);
    assert!(nav.iter().any(|n| n.title == "Academic History & Transcripts"), "entities decoded");
}

#[test]
fn questions_read_their_current_answers() {
    let p = page::page(HREF, PAGE);
    assert_eq!(p.title, "Basic Information");
    assert_eq!(current(q(&p, "sys:first")), Some(one("Pat")));
    assert_eq!(q(&p, "sys:first").label, "Legal First Name *");
    assert_eq!(current(q(&p, "sys:field:gd_lived_name_diff_legal_name")), Some(one("No")));
    assert_eq!(current(q(&p, "sys:field:gd_race_ethnicity")), Some(Answer::Many(vec!["Asian American/Asian".into()])));
    assert_eq!(current(q(&p, "sys:birthdate")), Some(one("2000-01-03")));
    assert_eq!(current(q(&p, "sys:field:gd_birth_country")), Some(one("United States")));
    assert_eq!(current(q(&p, "sys:app:gd_parent1_highest_edu")), None, "nothing selected");
    let sop = q(&p, "sys:app:gd_statement_of_purpose");
    assert_eq!((sop.kind, sop.required, sop.max_words), (Kind::Textarea, true, Some(500)));
}

#[test]
fn formula_copies_and_secrets_are_never_written() {
    let p = page::page(HREF, PAGE);
    let copies: Vec<_> =
        p.questions.iter().filter(|q| q.export.as_deref() == Some("sys:field:gd_lived_first")).collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(copies.iter().filter(|q| q.writable()).count(), 1, "the data-readonly formula copy is skipped");
    let ssn = q(&p, "sys:ssn");
    assert_eq!(ssn.kind, Kind::Other);
    assert!(!ssn.writable());
}

#[test]
fn plan_posts_only_what_differs() {
    let want = answers(&[
        ("sys:first", one("Pat")),                                   // already so
        ("sys:field:gd_lived_name_diff_legal_name", one("yes")),     // case-insensitive choice
        ("sys:birthdate", one("2000-01-03")),                        // already so
        ("sys:field:gd_lived_first", one("Pat")),                    // shared key: the editable one
        ("sys:app:gd_parent1_highest_edu", one("Some High School")), // blank -> set
    ]);
    let plan = answer::plan(&pages(), &want.into());
    assert!(plan.findings.is_empty(), "{:?}", plan.findings);
    let keys: Vec<&str> = plan.changes.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["sys:app:gd_parent1_highest_edu", "sys:field:gd_lived_name_diff_legal_name"]);
    let radio = &plan.changes[1];
    assert_eq!(radio.fields, vec![("form_23b97894-f8ed-4d54-8746-27edd1d2b133".to_string(), "1".to_string())]);
    assert_eq!(radio.from, Some(one("No")));
}

#[test]
fn dates_and_checkboxes_encode_every_part() {
    let p = page::page(HREF, PAGE);
    let (differs, f) = answer::encode(q(&p, "sys:birthdate"), &one("2000-02-03")).expect("encodes");
    assert!(differs);
    let names: Vec<&str> = f.iter().map(|(n, _)| n.rsplit('_').next().unwrap_or_default()).collect();
    assert_eq!(names, ["m", "d", "y"]);
    assert_eq!(f[0].1, "02");
    let race = Answer::Many(vec!["White/Caucasian".into(), "Asian American/Asian".into()]);
    let (differs, f) = answer::encode(q(&p, "sys:field:gd_race_ethnicity"), &race).expect("encodes");
    assert!(differs);
    assert_eq!(f.len(), 2, "one field per checked box");
    assert!(f.iter().all(|(n, _)| n == "form_15c93cfd-8305-414a-b928-4b4655752871"));
}

#[test]
fn unwritable_answers_are_violations() {
    let long = vec!["word"; 501].join(" ");
    let want = answers(&[
        ("sys:no_such_key", one("x")),
        ("sys:birthdate", one("1999-01-03")), // year not offered
        ("sys:field:gd_birth_country", one("Atlantis")),
        ("sys:app:gd_statement_of_purpose", one(&long)),
        ("sys:ssn", one("000000000")),
        ("sys:field:gd_race_ethnicity", Answer::Many(vec![])),
    ]);
    let plan = answer::plan(&pages(), &want.into());
    assert!(plan.changes.is_empty());
    assert_eq!(plan.findings.len(), 6, "{:#?}", plan.findings);
    assert!(plan.findings.iter().all(Finding::is_violation));
    let text = plan.findings.iter().map(ToString::to_string).collect::<String>();
    assert!(text.contains("501 words, the limit is 500"), "{text}");
    assert!(text.contains("\"Atlantis\" is not a choice"), "{text}");
}

#[test]
fn pulled_answers_plan_to_nothing() {
    // The statement's label is empty (visually hidden): answered here so its
    // line is written too.
    let pages = vec![page::page(HREF, &PAGE.replace("></textarea>", ">Line one.\nLine \"two\".</textarea>"))];
    let toml_text = answer::to_toml(&pages, "# pulled");
    assert!(toml_text.contains("sys:app:gd_statement_of_purpose"), "{toml_text}");
    let file: AnswersFile = toml::from_str(&toml_text).expect("pulled file parses");
    assert!(file.answers.len() >= 6, "{toml_text}");
    assert!(!file.answers.contains_key("sys:ssn"));
    assert!(!toml_text.lines().any(|l| l.trim_end().ends_with('#')), "no empty label comments:\n{toml_text}");
    let plan = answer::plan(&pages, &file);
    assert!(plan.changes.is_empty() && plan.findings.is_empty(), "{}", plan.render());
}

#[test]
fn review_splits_errors_from_warnings() {
    let issues = page::review(REVIEW).expect("review page");
    let errors: Vec<_> = issues.iter().filter(|i| i.error).collect();
    assert_eq!(errors.len(), 14);
    assert_eq!(errors[0].section, "Basic Information");
    assert_eq!(errors[0].message, "Basic Information - Missing Daytime Telephone Number");
    assert_eq!(errors[0].href, HREF);
    let warnings: Vec<_> = issues.iter().filter(|i| !i.error).collect();
    assert!(!warnings.is_empty());
    assert!(warnings[0].message.starts_with("Social Security Number (SSN) is missing"), "{:?}", warnings[0]);
}

#[test]
fn collateral_reports_answers_that_moved() {
    let before = page::page(HREF, PAGE);
    let after = page::page(HREF, &PAGE.replace("value=\"Pat\"", "value=\"\""));
    let parent = q(&before, "sys:app:gd_parent1_highest_edu").id.clone();
    let moved = admit_slate::collateral(Some(&before), &after, &BTreeSet::from([parent.as_str()]));
    assert!(moved.iter().any(|m| m.starts_with("sys:first \"Pat\" -> (blank)")), "{moved:?}");
    let first = q(&before, "sys:first").id.clone();
    let lived = before
        .questions
        .iter()
        .filter(|q| q.export.as_deref() == Some("sys:field:gd_lived_first"))
        .map(|q| q.id.as_str());
    let touched: BTreeSet<&str> = lived.chain([first.as_str()]).collect();
    assert!(admit_slate::collateral(Some(&before), &after, &touched).is_empty(), "touched questions may change");
}

#[test]
fn of_two_checked_radios_the_last_is_the_answer() {
    // Welcome page, captured 2026-10-06: the default and the stored choice
    // both carry checked="checked"; the browser shows (and posted) the last.
    let html = r#"<div class="form_question form_radio" id="form_question_85f54938-fd31-44e9-b67b-ffc2168b76b8" data-id="85f54938-fd31-44e9-b67b-ffc2168b76b8" data-type="radio" data-export="sys:field:gd_contact_preference"><div class="form_responses"><input id="form_85f54938-fd31-44e9-b67b-ffc2168b76b8_1" type="radio" data-text="Email correspondence" name="form_85f54938-fd31-44e9-b67b-ffc2168b76b8" checked="checked" value="7b04c358-064b-45cd-90c6-8dab027853ff" autocomplete="off" /><input id="form_85f54938-fd31-44e9-b67b-ffc2168b76b8_2" type="radio" data-text="Email and text when appropriate" name="form_85f54938-fd31-44e9-b67b-ffc2168b76b8" checked="checked" value="7d6c91a3-69cc-47a6-93d1-db6f885effb1" autocomplete="off" /></div></div>"#;
    let p = page::page("frm?w", html);
    let pref = q(&p, "sys:field:gd_contact_preference");
    assert_eq!(current(pref), Some(one("Email and text when appropriate")));
    let (differs, _) = answer::encode(pref, &one("Email and text when appropriate")).expect("encodes");
    assert!(!differs, "the shown answer is not a change");
}

#[test]
fn an_autocomplete_code_field_is_written_with_its_name() {
    // Survey Information, captured 2026-10-06: the institution name and its
    // data-invisible code, which the page's Suggest fills from
    // /manage/service/lookup. Without the code Review says "Institution not
    // recognized".
    let html = r#"<h1>Survey Information</h1><div class="form_question form_text" data-id="c9ab5f41-ffbf-4edc-adb8-f0c7f34d856f" data-type="text" data-export="sys:app:gd_survey_institution1"><input type="text" name="form_c9ab5f41-ffbf-4edc-adb8-f0c7f34d856f" value="" /></div><div class="form_question form_text hidden" data-id="8c0e80b3-cd6a-4f25-b601-cb7fc7b1bf95" data-type="text" data-export="sys:app:gd_survey_institutioncode1" data-invisible="1"><input type="text" name="form_8c0e80b3-cd6a-4f25-b601-cb7fc7b1bf95" value="" /></div>"#;
    let p = page::page("frm?survey", html);
    let want = answers(&[
        ("sys:app:gd_survey_institution1", one("Yale University")),
        ("sys:app:gd_survey_institutioncode1", one("003987")),
    ]);
    let plan = answer::plan(std::slice::from_ref(&p), &want.into());
    assert!(plan.findings.is_empty(), "{:?}", plan.findings);
    let fields: Vec<(String, String)> = plan.changes.iter().flat_map(|c| c.fields.clone()).collect();
    assert!(fields.contains(&("form_8c0e80b3-cd6a-4f25-b601-cb7fc7b1bf95".into(), "003987".into())), "{fields:?}");
}

#[test]
fn a_year_month_date_has_no_day_part() {
    // A school's From (data-type="dateym"), live 2026-10-06: month and year selects only.
    let html = r#"<h1>School</h1><div class="form_question form_dateym" data-id="f1" data-type="dateym" data-export="sys:school:from" data-required="1"><select name="form_f1_m"><option value=""></option><option value="08" selected="selected">August</option><option value="09">September</option></select><select name="form_f1_y"><option value=""></option><option value="2025" selected="selected">2025</option></select></div>"#;
    let p = page::page("frm?s", html);
    let q = &p.questions[0];
    assert_eq!(current(q), Some(one("2025-08")));
    let (differs, f) = answer::encode(q, &one("2025-09")).expect("encodes");
    assert!(differs);
    assert_eq!(f, vec![("form_f1_m".to_string(), "09".to_string()), ("form_f1_y".to_string(), "2025".to_string())]);
    assert!(answer::encode(q, &one("2025-09-01")).is_err(), "no day part to set");
}

#[test]
fn answer_boundaries_reject_ambiguous_and_unrepresentable_values() {
    use admit_slate::{Answer, answer, page};
    use std::collections::BTreeMap;
    let html = "<div class=\"form_question\" data-id='a' data-type='text' data-export='same'><input name='form_a' maxlength='2'></div><div class=\"form_question\" data-id='b' data-type='text' data-export='same'><input name='form_b'></div>";
    let p = page::page("frm?x", html);
    let want = BTreeMap::from([("same".into(), Answer::One("value".into()))]).into();
    let plan = answer::plan(std::slice::from_ref(&p), &want);
    assert!(plan.findings.iter().any(|f| f.to_string().contains("share that key")));
    assert!(
        answer::encode(&p.questions[0], &Answer::Many(vec!["x".into()]))
            .expect_err("single value")
            .contains("not a list")
    );
    assert!(answer::encode(&p.questions[0], &Answer::One("abc".into())).expect_err("length").contains("takes 2"));
    let other = page::page("frm?x", "<div class=\"form_question\" data-id='q' data-type='unknown'></div>");
    assert!(answer::encode(&other.questions[0], &Answer::One("x".into())).is_err());
    assert_eq!(Answer::Many(vec!["a".into(), "b".into()]).to_string(), "[\"a\", \"b\"]");
    let want = BTreeMap::from([("b".into(), Answer::One("x".repeat(200)))]).into();
    assert!(answer::plan(&[p], &want).render().contains("202 chars)"));
}

#[test]
fn answering_two_questions_that_share_an_export_key_is_refused() {
    // UCLA's Award Application: Cota-Robles and GOP both store sys:app:gd_div_fellowship.
    use admit_slate::{Answer, answer, page};
    use std::collections::BTreeMap;
    let radio = |id: &str, yes: &str| {
        format!(
            "<div class=\"form_question\" data-id='{id}' data-type='radio' data-export='sys:app:gd_div_fellowship'><input type='radio' name='form_{id}' value='{id}-y' data-text='{yes}'><input type='radio' name='form_{id}' value='{id}-n' data-text='No'></div>"
        )
    };
    let html = radio("cota", "Cota")
        + &radio("gop", "GOP")
        + "<div class=\"form_question\" data-id='t' data-type='text' data-export='t'><input name='form_t'></div>";
    let p = page::page("frm?x", &html);
    let both = BTreeMap::from([
        ("cota".into(), Answer::One("Cota".into())),
        ("gop".into(), Answer::One("GOP".into())),
        ("t".into(), Answer::One("kept".into())),
    ]);
    let plan = answer::plan(std::slice::from_ref(&p), &both.into());
    assert_eq!(plan.findings.len(), 1, "{:?}", plan.findings);
    let f = plan.findings[0].to_string();
    assert!(f.contains("\"cota\", \"gop\"") && f.contains("gd_div_fellowship"), "{f}");
    let keys: Vec<&str> = plan.changes.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, ["t"], "neither clashing answer is written; the rest still are");

    let one = BTreeMap::from([("gop".into(), Answer::One("GOP".into()))]);
    let plan = answer::plan(std::slice::from_ref(&p), &one.into());
    assert!(plan.findings.is_empty(), "{:?}", plan.findings);
    assert_eq!(plan.changes.len(), 1);
}

#[test]
fn checkbox_single_answers_and_bad_full_dates_have_explicit_encodings() {
    let p = page::page("frm?x", PAGE);
    let checkbox = p.questions.iter().find(|q| q.kind == page::Kind::Checkbox).expect("checkbox");
    let (_, fields) = answer::encode(checkbox, &Answer::One("Asian American/Asian".into())).expect("one checkbox");
    assert_eq!(fields.len(), 1);
    let date = p.questions.iter().find(|q| q.kind == page::Kind::Date).expect("date");
    assert!(answer::encode(date, &Answer::One("2000-01".into())).expect_err("missing day").contains("not YYYY-MM-DD"));
}

#[test]
fn unknown_choice_values_and_shared_exports_survive_pull() {
    let html = r#"<div class="form_question" data-id="a" data-type="radio" data-export="same"><input name="form_a" type="radio" value="1" checked></div><div class="form_question" data-id="b" data-type="text" data-export="same"><input name="form_b" value="two"></div><div class="form_question" data-id="c" data-type="checkbox"><input name="form_c" type="checkbox" value="1" checked></div>"#;
    let mut p = page::page("frm?x", html);
    p.questions[0].inputs[0].values = vec!["unrecognized".into()];
    assert_eq!(current(&p.questions[0]), Some(Answer::One("unrecognized".into())));
    let pulled = answer::to_toml(&[p], "# test");
    let file: admit_slate::AnswersFile = toml::from_str(&pulled).expect("pull");
    assert_eq!(file.answers["a"], Answer::One("unrecognized".into()));
    assert_eq!(file.answers["b"], Answer::One("two".into()));
    assert!(matches!(file.answers["c"], Answer::Many(_)));
    assert!(page::applications("<tr data-href='?id=&amp;cmd=detail'><td>bad</td></tr>").is_empty());
}

/// A pull must be a file `plan` can read back: a key holding a newline was
/// written as a `"""` string (not a legal key) and a control character in a
/// label broke its comment (2026-10-07 fuzz run).
#[test]
fn pull_writes_parseable_toml_for_any_key_or_label() {
    let html = "<h1>Basic\u{14} Information</h1><div class=\"form_question\" data-id=\"a\" data-type=\"text\" data-export=\"odd&#10;&quot;key\\\"><label class=\"form_label\">Name\u{1}\u{7f}</label><input name=\"form_a\" value=\"v\"></div>";
    let pages = vec![page::page("frm?x", html)];
    let pulled = answer::to_toml(&pages, "# test");
    let file: AnswersFile = toml::from_str(&pulled).unwrap_or_else(|e| panic!("{e}\n{pulled}"));
    assert_eq!(file.answers["odd\n\"key\\"], Answer::One("v".into()));
    let plan = answer::plan(&pages, &file);
    assert!(plan.changes.is_empty() && plan.findings.is_empty(), "{:?} {:?}", plan.changes, plan.findings);
}
