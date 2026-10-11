use super::*;

#[test]
fn text_drops_tags_and_collapses_space() {
    assert_eq!(text("Race/Ethnicity<br />\nIn  addition, &amp; more"), "Race/Ethnicity In addition, & more");
}

#[test]
fn options_skip_the_blank_and_read_selection() {
    let (c, v) = options(
        r#"<option></option><option value="US" data-text="United States" selected="selected">United States</option><option value="CA">Canada</option>"#,
    );
    assert_eq!(c.len(), 2);
    assert_eq!(c[0], Choice { value: "US".into(), text: "United States".into() });
    assert_eq!(v, vec!["US".to_string()]);
}

/// A bare `selected` is selected in HTML, like a bare `checked`.
#[test]
fn a_bare_selected_option_is_selected() {
    let (_, v) = options(r#"<option value="1">One</option><option value="2" selected>Two</option>"#);
    assert_eq!(v, vec!["2".to_string()]);
}

#[test]
fn review_refuses_a_page_that_is_not_review() {
    assert_eq!(review("<h1>Basic Information</h1>"), None);
    assert_eq!(review("<h1>Review</h1><p>no tables</p>"), Some(vec![]));
    assert_eq!(review("<h1>Review</h1><tbody><tr><td>only</td></tr></tbody>"), Some(vec![]));
}

#[test]
fn truncated_markup_is_skipped_rather_than_invented() {
    assert!(address_widget("class=\"widget_add\"").is_none());
    assert!(address_widget("< class=\"widget_add\"").is_none());
    assert!(address_widget("<a class=\"widget_add\">").is_none());
    let short = address_widget(
        "<a class=\"widget_add\" data-href=\"/apply/frm?add\"><tr class=\"widget_table_row\"><td>Mailing</td></tr>",
    )
    .expect("widget");
    assert!(short.rows.is_empty());

    let nav_html = "<div id=\"menu\"><a href=\"frm?ok\">Ok</a><a href=\"frm?bad\"";
    assert_eq!(nav(nav_html).len(), 1);

    let questions = page(
        "frm?x",
        "<div class=\"form_question\" data-id=\"ok\" data-type=\"text\"><input name=\"form_ok\" value=\"a\"></div><div class=\"form_question\"",
    );
    assert_eq!(questions.questions.len(), 1);
    assert!(page("frm?m", "<div class=\"form_question\" data-type=\"text\"></div>").questions.is_empty());
    assert_eq!(page("frm?h", "<h1>No close").title, "");

    let labeled = page(
        "frm?l",
        "<div class=\"form_question\" data-id=\"q\" data-type=\"text\"><label class=\"form_label\"<input name=\"form_q\" value=\"x\"></div>",
    );
    assert_eq!(labeled.questions[0].label, "");
    assert!(labeled.questions[0].max_words.is_none());

    let words = page(
        "frm?w",
        "<div class=\"form_question\" data-id=\"t\" data-type=\"textarea\"><textarea name=\"form_t\" data-maxlength=\"chars:10\"></textarea></div>",
    );
    assert_eq!(words.questions[0].max_words, None);

    let radio = page(
        "frm?r",
        "<div class=\"form_question\" data-id=\"r\" data-type=\"radio\"><input type=\"radio\" name=\"form_r\" value=\"yes\" checked=\"checked\"></div>",
    );
    assert_eq!(current_text(&radio), "yes");

    let select = page(
        "frm?s",
        "<div class=\"form_question\" data-id=\"s\" data-type=\"select\"><select name=\"form_s\"><option value=\"1\" selected>One</option><option value=\"2\"</select></div>",
    );
    assert_eq!(select.questions[0].main_input().expect("select").choices.len(), 1);

    assert!(widget_rows("<tr data-href=\"/apply/frm?cmd=widget&id=1\"").is_empty());
    assert!(widget_rows("<tr <a data-href=\"/apply/frm?cmd=widget&id=1\"").is_empty());
    assert!(widget_rows("<tr><br> data-href=\"/apply/frm?cmd=widget&id=1\"").is_empty());

    assert_eq!(jobs_listed("not the job page"), None);
    assert_eq!(jobs_listed("<h1>Professional Experience</h1>"), None);
    assert_eq!(
        jobs_listed("<h1>Professional Experience</h1><div id=\"part_table\"><table></table></div>"),
        Some(vec![])
    );
    let jobs = jobs_listed(
        "<h1>Professional Experience</h1><div id=\"part_table\"><table><tbody><tr></tr><tr><td>Only</td></tr><tr><td>Clinic</td><td>01/2020–Present</td></tr></tbody></table></div>",
    )
    .expect("jobs");
    assert_eq!(jobs, vec![("Clinic".into(), "01/2020–Present".into())]);

    assert_eq!(institutions_listed("<h1>Academic History</h1>"), None);
    assert_eq!(
        institutions_listed("<h1>Academic History</h1><div id=\"part_table\"><table></table></div>"),
        Some(vec![])
    );

    let degrees = degree_choices(
        "<div class=\"hidden level level_U\">nope</div><div class=\"hidden level level_G\"><select name=\"degree\"><option value=\"ba\">BA</option></select></div>",
    );
    assert_eq!(degrees.keys().map(String::as_str).collect::<Vec<_>>(), ["G"]);

    let lists = checklists(
        "<div id=\"part_checklist_App\"><h3>Application</h3><table><tr class=\"plain\"><td>icon</td><td>Received</td></tr><tr class=\"plain\"><td>icon</td><td>Awaiting</td><td>Essay</td><td></td></tr></table></div>",
    );
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].detail, "Essay");

    assert_eq!(update_posted("was last posted June 2, 2026"), None);
    assert_eq!(update_pdf("id=\"pager_download\""), None);
    assert_eq!(update_pdf("<id=\"pager_download\""), None);
    assert_eq!(update_pdf("<a id=\"pager_download\">"), None);

    assert!(applications("<tr data-href='detail'><td>Name</td></tr>").is_empty());
}

/// Inputs the 2026-10-07 fuzz run found panicking: a slice whose end was
/// searched for from before its start, or a start inside a multibyte char.
#[test]
fn closers_found_before_the_opener_do_not_panic() {
    let select = page(
        "frm?s",
        "<div class=\"form_question\" data-id=\"s\" data-type=\"select\"><select name=\"form_s\" data-x=\"</select>\"><option value=\"1\" selected=\"selected\">One</option></select></div>",
    );
    assert_eq!(current_text(&select), "One");

    let degrees = degree_choices(
        "<div class=\"hidden level level_U\"></select><select name=\"degree\"><option value=\"ms\">MS</option></select></div>",
    );
    assert_eq!(degrees["U"], vec![Choice { value: "ms".into(), text: "MS".into() }]);

    let apps = applications("<tr\u{2003}data-href=\"?id=abc&amp;cmd=detail\"><td>Nursing</td><td>Open</td></tr>");
    assert_eq!(apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["abc"]);
}

fn current_text(p: &Page) -> String {
    match crate::answer::current(&p.questions[0]) {
        Some(crate::answer::Answer::One(s)) => s,
        other => panic!("expected one answer, got {other:?}"),
    }
}

#[test]
fn errors_reads_each_error_paragraph_and_nothing_else() {
    // The markup of captures/ucla/live_login.html, Slate's refusal page.
    let page = "<h1>Error</h1><p class=\"error\">This browser does not appear to be configured to accept cookies.</p>\
                <p class=\"warning\">not this</p><p class=\"error\"> Invalid <b>password</b>. </p><p class=\"error\"></p>";
    assert_eq!(
        errors(page),
        ["This browser does not appear to be configured to accept cookies.", "Invalid password ."]
    );
    // A closer before the opener's `>` (or none) reads as nothing, not a panic.
    assert!(errors("<p class=\"error\"</p>").is_empty());
    assert!(errors("<p class=\"error\"").is_empty(), "no `>` at all");
    assert!(errors("<p class=\"error\">unclosed").is_empty());
}
