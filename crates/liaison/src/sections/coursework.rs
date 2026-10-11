//! Academic History → Transcript Entry, driven by parsed transcripts.
//!
//! Each transcript course is matched against the coursework already entered
//! for the same institution (same normalized code OR same normalized title,
//! in any term). Unmatched courses are added, creating their term first when
//! the institution has none for that season and year. Grade differences are
//! reported, never written: posting grades to an in-progress term goes
//! through a request this tool has not yet seen succeed.
//!
//! Two more transcript-driven changes:
//! - a portal course whose code is mistyped (same term, title abbreviates the
//!   transcript's) is corrected in place ([`CourseEdit`]);
//! - an institution's "still attending" flag follows the transcript: true
//!   while it lists an ungraded course there, otherwise false with the
//!   attendance ending at the last term listed ([`AttendanceFix`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use admit_domain::{Finding, TranscriptSource};
use admit_sources::{TranscriptCourse, normalize};
use serde_json::{Value, json};

use crate::client::{ApiError, Client, Result, arr, int, s};
use crate::lookup::Lookups;

#[derive(Debug, Clone, PartialEq)]
pub enum TermTarget {
    Existing { id: i64, label: String },
    New { term: String, year: String, in_progress: bool, academic_status: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CourseAdd {
    pub college_id: i64,
    pub college: String,
    pub college_code: String,
    pub target: TermTarget,
    pub course: TranscriptCourse,
    pub subject: String,
}

/// A portal institution with its entered terms and courses.
#[derive(Debug, Clone)]
pub struct LiveCollege {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub terms: Vec<Value>,
    /// The `/academicHistory/courseWork` entry (`stillAttending`,
    /// `collegeEndTerm`, `collegeEndYear`, …).
    pub info: Value,
}

/// Correct a portal course in place, the way the course form's Save does
/// (`PUT /academicHistory/courses/collegeId/{c}/termId/{t}/courseId/{id}`,
/// read from the applicant bundle 2026-10; body = the create form's fields).
#[derive(Debug, Clone, PartialEq)]
pub struct CourseEdit {
    pub college_id: i64,
    pub college: String,
    pub term_id: i64,
    pub term: String,
    pub course_id: i64,
    /// (code, title) on the portal now, and as the transcript prints them.
    pub from: (String, String),
    pub to: (String, String),
    /// (portal subject now, subject to set), when the subject changes.
    pub subject: Option<(String, String)>,
    pub body: Value,
}

/// Set an institution's "still attending" flag (and, when false, its last
/// term) from the transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct AttendanceFix {
    pub college_id: i64,
    pub college: String,
    pub still_attending: bool,
    /// Last (season, year) with coursework there, and that session's end month.
    pub last: (String, String),
    pub end_month: u8,
    /// What the portal says now, for the plan.
    pub was: String,
}

pub async fn live(c: &Client) -> Result<Vec<LiveCollege>> {
    let mut out = Vec::new();
    for col in arr(&c.get("/academicHistory/courseWork").await?) {
        let id = int(&col["id"]);
        let summary = c.get(&format!("/academicHistory/courseWork/summary/{id}")).await?;
        out.push(LiveCollege {
            id,
            name: s(&col["college"]),
            code: s(&col["collegeCode"]),
            terms: arr(&summary["terms"]).to_vec(),
            info: col.clone(),
        });
    }
    Ok(out)
}

pub fn read(src: &TranscriptSource, profile_dir: &Path) -> std::result::Result<Vec<TranscriptCourse>, String> {
    let path = profile_dir.join(&src.path);
    let labels: Vec<&str> = src.colleges.keys().map(String::as_str).collect();
    admit_sources::parse_file(&src.parser, &path, &labels).map_err(|e| format!("{}: {e}", path.display()))
}

fn dept(code: &str) -> &str {
    code.rsplit_once(' ').map_or(code, |(d, _)| d)
}

fn season_matches(portal_term: &str, season: &str) -> bool {
    portal_term == season || (season == "Summer" && portal_term.starts_with("Summer"))
}

fn blank_grade(g: &Value) -> String {
    let g = s(g);
    if g == "None" { String::new() } else { g }
}

/// One title abbreviates the other ("SPORT & EXERCISE" / "SPORT & EXERCISE PSYCHOLOGY").
fn abbreviates(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    a.len().min(b.len()) >= 6 && (a.starts_with(&b) || b.starts_with(&a))
}

/// An institution to add because a transcript shows coursework there.
#[derive(Debug, Clone, PartialEq)]
pub struct CollegeCreate {
    pub name: String,
    /// First and last (season, year) of coursework there.
    pub first: (String, String),
    pub last: (String, String),
    /// Start month of the first session and end month of the last (1-12).
    pub months: (u8, u8),
    pub still_attending: bool,
}

/// Season order within a year, for first/last term.
/// Order within a year; portal summer sessions are "Summer 1"/"Summer 2".
pub(crate) fn season_rank(s: &str) -> u8 {
    match s {
        "Winter" => 0,
        "Spring" => 1,
        x if x.starts_with("Summer") => 2,
        _ => 3,
    }
}

/// Typical months when the transcript doesn't print session dates.
fn default_months(season: &str) -> (u8, u8) {
    match season {
        "Winter" => (1, 2),
        "Spring" => (2, 6),
        "Summer" => (6, 8),
        _ => (8, 12),
    }
}

fn college_create(name: &str, courses: &[&TranscriptCourse]) -> Option<CollegeCreate> {
    // `min` and `max` are both `None` only for an empty slice, so a second `?`
    // after the first succeeded could never fail.
    let rank = |c: &TranscriptCourse| (c.year.clone(), season_rank(&c.season));
    let mut first = *courses.first()?;
    let mut last = first;
    for c in courses.iter().copied() {
        if rank(c) < rank(first) {
            first = c;
        }
        if rank(c) >= rank(last) {
            last = c;
        }
    }
    Some(CollegeCreate {
        name: name.to_string(),
        first: (first.season.clone(), first.year.clone()),
        last: (last.season.clone(), last.year.clone()),
        months: (
            first.term_months.unwrap_or_else(|| default_months(&first.season)).0,
            last.term_months.unwrap_or_else(|| default_months(&last.season)).1,
        ),
        still_attending: courses.iter().any(|c| c.grade.is_empty()),
    })
}

/// Everything the transcripts imply for the portal.
#[derive(Debug, Default)]
pub struct CourseworkPlan {
    pub colleges: Vec<CollegeCreate>,
    pub adds: Vec<CourseAdd>,
    pub edits: Vec<CourseEdit>,
    pub attendance: Vec<AttendanceFix>,
    pub findings: Vec<Finding>,
}

/// Pure planning step (tested without a portal).
///
/// Each portal course matches at most one transcript row (repeats like a
/// withdrawn-then-retaken course are separate rows), preferring the same
/// term: same code, then same title, then the same in any term. A row with no
/// match but a same-term portal course whose title it abbreviates (or vice
/// versa) is a likely mistyped code: reported, never added as a duplicate.
pub fn plan(
    src: &TranscriptSource,
    courses: &[TranscriptCourse],
    colleges: &[LiveCollege],
    subject_id: impl Fn(&str) -> Option<i64>,
) -> CourseworkPlan {
    // Each closure type monomorphizes this body, and region coverage scores
    // only one of those copies. Forwarding to one shared body lets every
    // caller cover the same regions.
    plan_with(src, courses, colleges, &subject_id)
}

fn plan_with(
    src: &TranscriptSource,
    courses: &[TranscriptCourse],
    colleges: &[LiveCollege],
    subject_id: &dyn Fn(&str) -> Option<i64>,
) -> CourseworkPlan {
    let mut out = CourseworkPlan::default();
    // Department prefix → subjects already used for it on the portal.
    let mut dept_subjects: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for col in colleges {
        for t in &col.terms {
            for x in arr(&t["courses"]) {
                dept_subjects.entry(normalize(dept(&s(&x["code"])))).or_default().insert(s(&x["subject"]));
            }
        }
    }
    let skip: BTreeSet<String> = src.skip.iter().map(|c| normalize(c)).collect();
    let mut taken: BTreeSet<i64> = BTreeSet::new();
    let mut missing: BTreeMap<String, Vec<&TranscriptCourse>> = BTreeMap::new();

    for c in courses.iter().filter(|c| !skip.contains(&normalize(&c.code))) {
        let Some(name) = src.colleges.get(&c.college) else { continue };
        let Some(col) = colleges.iter().find(|l| l.name.eq_ignore_ascii_case(name)) else {
            missing.entry(name.clone()).or_default().push(c);
            continue;
        };
        let same_term = |t: &Value| s(&t["year"]) == c.year && season_matches(&s(&t["term"]), &c.season);
        let candidates = |pred: &dyn Fn(&Value, &Value) -> bool| {
            col.terms.iter().find_map(|t| {
                arr(&t["courses"]).iter().find(|x| !taken.contains(&int(&x["id"])) && pred(t, x)).map(|x| (t, x))
            })
        };
        let code_eq = |x: &Value| normalize(&s(&x["code"])) == normalize(&c.code);
        let title_eq = |x: &Value| normalize(&s(&x["title"])) == normalize(&c.title);
        let matched = candidates(&|t, x| same_term(t) && code_eq(x))
            .or_else(|| candidates(&|t, x| same_term(t) && title_eq(x)))
            .or_else(|| candidates(&|_, x| code_eq(x)))
            .or_else(|| candidates(&|_, x| title_eq(x)));
        if let Some((t, x)) = matched {
            taken.insert(int(&x["id"]));
            let what = format!("{name} {} {} {}", s(&t["term"]), s(&t["year"]), s(&x["code"]));
            let live_grade = blank_grade(&x["grade"]);
            if live_grade != c.grade {
                out.findings.push(Finding::Pending(
                    if live_grade.is_empty() && t["termInProgress"].as_bool() == Some(true) {
                        format!(
                            "{what}: grade {} has posted; complete the term and enter grades in the portal",
                            c.grade
                        )
                    } else {
                        format!("{what}: portal grade {:?} vs transcript {:?}", live_grade, c.grade)
                    },
                ));
            }
            // Corrections to a matched course: the transcript's code when the
            // portal's differs (matched by title in the same term, e.g. "POL
            // SCI" for "POL SCI 019"), and the subject coursework.toml names
            // for this exact code.
            let code = (!code_eq(x) && same_term(t)).then(|| c.code.clone());
            let subject = src
                .subjects
                .get(&c.code)
                .filter(|w| !w.eq_ignore_ascii_case(&s(&x["subject"])))
                .and_then(|w| Some((w.clone(), subject_id(w)?)));
            if src.subjects.get(&c.code).is_some_and(|w| subject_id(w).is_none()) {
                out.findings.push(Finding::Violation(format!(
                    "{what}: subject {:?} in coursework.toml is not a portal subject",
                    src.subjects.get(&c.code).cloned().unwrap_or_default()
                )));
            }
            let fix_title = !title_eq(x) && src.fix_titles.iter().any(|k| normalize(k) == normalize(&c.code));
            // Misspelled words in the portal title, caught against the
            // transcript's wording and corrected word by word (the rest of
            // the portal title, often fuller than the transcript's, stays).
            let typos = if fix_title { vec![] } else { title_typos(&s(&x["title"]), &c.title) };
            for (wrong, right) in &typos {
                out.findings.push(Finding::Violation(format!(
                    "{what}: title typo {wrong:?} (transcript has {right:?}) - corrected by `admit apply`"
                )));
            }
            if code.is_some() || subject.is_some() || fix_title || !typos.is_empty() {
                let to_code = code.unwrap_or_else(|| s(&x["code"]));
                let to_title = if fix_title { c.title.clone() } else { replace_words(&s(&x["title"]), &typos) };
                out.edits.push(course_edit(col, t, x, (to_code, to_title), subject));
            }
            if !fix_title && typos.is_empty() && !title_eq(x) && !abbreviates(&s(&x["title"]), &c.title) {
                out.findings.push(Finding::Note(format!(
                    "{what}: portal title {:?}, transcript {:?}",
                    s(&x["title"]),
                    c.title
                )));
            }
            continue;
        }
        if let Some((t, x)) = candidates(&|t, x| same_term(t) && abbreviates(&s(&x["title"]), &c.title)) {
            taken.insert(int(&x["id"]));
            out.edits.push(course_edit(col, t, x, (c.code.clone(), c.title.clone()), None));
            continue;
        }
        let subject = src.subjects.get(&c.code).or_else(|| src.subjects.get(dept(&c.code))).cloned().or_else(|| {
            let known = dept_subjects.get(&normalize(dept(&c.code)))?;
            (known.len() == 1).then(|| known.iter().next().cloned()).flatten()
        });
        let Some(subject) = subject.filter(|x| subject_id(x).is_some()) else {
            out.findings.push(Finding::Violation(format!(
                "{} {}: no valid subject - add it to [transcript.subjects] in coursework.toml",
                c.college, c.code
            )));
            continue;
        };
        let existing = col.terms.iter().find(|t| same_term(t));
        let target = match existing {
            Some(t) => {
                TermTarget::Existing { id: int(&t["id"]), label: format!("{} {}", s(&t["term"]), s(&t["year"])) }
            }
            None => TermTarget::New {
                term: if c.season == "Summer" { src.summer_term.clone() } else { c.season.clone() },
                year: c.year.clone(),
                in_progress: courses
                    .iter()
                    .filter(|o| o.college == c.college && o.year == c.year && o.season == c.season)
                    .all(|o| o.grade.is_empty()),
                academic_status: src.academic_status.clone(),
            },
        };
        out.adds.push(CourseAdd {
            college_id: col.id,
            college: col.name.clone(),
            college_code: col.code.clone(),
            target,
            course: c.clone(),
            subject,
        });
    }
    out.attendance = attendance(src, courses, colleges);
    for (name, cs) in missing {
        if src.create_missing_colleges {
            out.colleges.extend(college_create(&name, &cs));
        } else {
            out.findings.push(Finding::Pending(format!(
                "{name} is not on the application but the transcript has {} course(s) there \
                 (add it in the portal, or set create_missing_colleges = true)",
                cs.len()
            )));
        }
    }
    out
}

/// Edit distance (insert, delete, substitute), for short words.
fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

fn title_words(t: &str) -> Vec<String> {
    t.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| w.len() >= 4).map(str::to_ascii_uppercase).collect()
}

/// Words of the portal title that are misspellings of a transcript word:
/// within a fifth of the word's length in edits (at least one), and not an
/// abbreviation of each other (one a prefix of the other: PSYCHOLOG /
/// PSYCHOLOGY, INTRO / INTRODUCTION). Returns (portal word, transcript word)
/// as the portal spells it, with the correction in the portal's case.
pub fn title_typos(portal: &str, transcript: &str) -> Vec<(String, String)> {
    let theirs = title_words(transcript);
    let mut out: Vec<(String, String)> = Vec::new();
    for w in portal.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| w.len() >= 4) {
        let up = w.to_ascii_uppercase();
        if theirs.contains(&up) || out.iter().any(|(x, _)| x == w) {
            continue;
        }
        let limit = (up.len() / 5).max(1);
        let best = theirs
            .iter()
            .filter(|t| !t.starts_with(&up) && !up.starts_with(t.as_str()))
            // The transcript dropping letters is an abbreviation ("SKLLS"
            // for SKILLS), not a portal typo.
            .filter(|t| !is_subsequence(t, &up))
            .map(|t| (levenshtein(&up, t), t))
            .filter(|(d, _)| *d <= limit)
            .min_by_key(|(d, _)| *d);
        if let Some((_, t)) = best {
            let fixed = if w.chars().all(|c| c.is_ascii_uppercase()) {
                t.clone()
            } else {
                let lower = t.to_ascii_lowercase();
                let mut cs = lower.chars();
                cs.next().map(|f| f.to_ascii_uppercase().to_string() + cs.as_str()).unwrap_or_default()
            };
            out.push((w.to_string(), fixed));
        }
    }
    out
}

/// `short`'s letters appear in `long` in order (`long` minus some letters).
fn is_subsequence(short: &str, long: &str) -> bool {
    let mut it = long.chars();
    short.chars().all(|c| it.any(|l| l == c))
}

/// `title` with each whole-word occurrence of a wrong word replaced.
fn replace_words(title: &str, fixes: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let w = std::mem::take(word);
        out.push_str(fixes.iter().find(|(x, _)| *x == w).map_or(w.as_str(), |(_, r)| r.as_str()));
    };
    for ch in title.chars() {
        if ch.is_ascii_alphabetic() {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn course_edit(
    col: &LiveCollege,
    t: &Value,
    x: &Value,
    to: (String, String),
    subject: Option<(String, i64)>,
) -> CourseEdit {
    let grade = blank_grade(&x["grade"]);
    let cls = &x["courseClassification"];
    let flag = |k: &str| json!(cls[k].as_bool() == Some(true));
    let body = json!({
        "code": to.0, "title": to.1, "grade": grade, "gradeEnabled": !grade.is_empty(),
        "convertedGrade": if s(&x["convertedGrade"]).is_empty() { json!("None") } else { x["convertedGrade"].clone() },
        "termId": int(&t["id"]).to_string(), "subjectId": subject.as_ref().map_or(int(&x["subjectId"]), |s| s.1).to_string(),
        "credits": s(&x["credits"]),
        "courseClassification": {
            "labSelected": flag("labSelected"), "honorsSelected": flag("honorsSelected"),
            "repeatedSelected": flag("repeatedSelected"), "advancedPlacementSelected": flag("advancedPlacementSelected"),
            "otherTestSelected": flag("otherTestSelected"), "studyAbroadSelected": flag("studyAbroadSelected"),
            "labOnlySelected": flag("labOnlySelected"), "labAndLectureSelected": flag("labAndLectureSelected"),
            "dpdProfessionalSelected": flag("dpdProfessionalSelected"), "dpdScienceSelected": flag("dpdScienceSelected"),
        },
        "source": x["source"],
    });
    CourseEdit {
        college_id: col.id,
        college: col.name.clone(),
        term_id: int(&t["id"]),
        term: format!("{} {}", s(&t["term"]), s(&t["year"])),
        course_id: int(&x["id"]),
        from: (s(&x["code"]), s(&x["title"])),
        to,
        subject: subject.map(|(name, _)| (s(&x["subject"]), name)),
        body,
    }
}

/// "Still attending" for every institution the transcript covers.
fn attendance(src: &TranscriptSource, courses: &[TranscriptCourse], colleges: &[LiveCollege]) -> Vec<AttendanceFix> {
    let mut out = Vec::new();
    for (label, name) in &src.colleges {
        let here: Vec<&TranscriptCourse> = courses.iter().filter(|c| &c.college == label).collect();
        let Some(col) = colleges.iter().find(|l| l.name.eq_ignore_ascii_case(name)) else { continue };
        let Some(last) = here.iter().max_by_key(|c| (c.year.clone(), season_rank(&c.season))) else { continue };
        let want = here.iter().any(|c| c.grade.is_empty());
        let live = col.info["stillAttending"].as_bool() == Some(true);
        let (end_term, end_year) = (s(&col.info["collegeEndTerm"]), s(&col.info["collegeEndYear"]));
        let ends_right = season_matches(&end_term, &last.season) && end_year == last.year;
        if want == live && (want || ends_right) {
            continue;
        }
        out.push(AttendanceFix {
            college_id: col.id,
            college: col.name.clone(),
            still_attending: want,
            last: (last.season.clone(), last.year.clone()),
            end_month: last.term_months.unwrap_or_else(|| default_months(&last.season)).1,
            was: if live { "still attending".into() } else { format!("ended {end_term} {end_year}") },
        });
    }
    out
}

/// Liaison month ids in calendar order (observed from `/lookup/month`).
const MONTH_IDS: [i64; 12] = [1, 2, 3, 4, 8, 9, 10, 11, 12, 13, 14, 15];

/// Adds an institution, as the "Add College" form does (captured 2026-10).
pub async fn create_college(c: &Client, lk: &Lookups, cc: &CollegeCreate) -> Result<()> {
    let found = c.get(&format!("/lookup/colleges?text={}&page=0&size=50", cc.name.replace(' ', "%20"))).await?;
    let code = arr(&found)
        .iter()
        .find(|x| s(&x["text"]).eq_ignore_ascii_case(&cc.name))
        .map(|x| s(&x["code"]))
        .ok_or_else(|| ApiError::Shape(format!("{:?} is not in the portal's institution list", cc.name)))?;
    let month = |m: u8| MONTH_IDS.get(usize::from(m.max(1)) - 1).copied().unwrap_or(1);
    let (start_term, start_year) = (lk.terms.id(&cc.first.0)?, lk.years.id(&cc.first.1)?);
    let mut body = json!({
        "countryId": "", "college": cc.name, "isPrimary": false,
        "termTypeId": lk.term_types.id("Semester")?, "stillAttending": cc.still_attending,
        "startTermId": start_term, "startMonthId": month(cc.months.0), "startYearId": start_year,
        "collegeStartTermId": start_term, "collegeStartMonthId": month(cc.months.0), "collegeStartYearId": start_year,
        "isDegree": "no", "collegeCode": code, "degrees": [], "degreeObtained": "no",
        "auEnabled": false, "optOut": false,
    });
    if !cc.still_attending {
        let (end_term, end_year) = (lk.terms.id(&cc.last.0)?, lk.years.id(&cc.last.1)?);
        for (k, v) in [("TermId", end_term), ("MonthId", month(cc.months.1)), ("YearId", end_year)] {
            body[format!("end{k}")] = json!(v);
            body[format!("collegeEnd{k}")] = json!(v);
        }
    }
    c.post("/academicHistory/colleges", &body).await.map(drop)
}

pub async fn apply_edits(c: &Client, edits: &[CourseEdit]) -> Result<()> {
    for e in edits {
        c.put(
            &format!(
                "/academicHistory/courses/collegeId/{}/termId/{}/courseId/{}",
                e.college_id, e.term_id, e.course_id
            ),
            &e.body,
        )
        .await?;
    }
    Ok(())
}

/// Re-saves each institution with the flag (and end term) changed, through
/// the same PUT the college form uses, keeping its degrees as they are.
pub async fn apply_attendance(c: &Client, lk: &Lookups, fixes: &[AttendanceFix]) -> Result<()> {
    if fixes.is_empty() {
        return Ok(());
    }
    let live = crate::education::live(c).await?;
    for f in fixes {
        let mut college = live
            .iter()
            .find(|x| int(&x["id"]) == f.college_id)
            .cloned()
            .ok_or_else(|| ApiError::Shape(format!("{} is no longer on the application", f.college)))?;
        college["stillAttending"] = json!(f.still_attending);
        if !f.still_attending {
            college["collegeEndTermId"] = json!(lk.terms.id(&f.last.0)?);
            college["collegeEndYearId"] = json!(lk.years.id(&f.last.1)?);
            college["collegeEndMonthId"] =
                json!(MONTH_IDS.get(usize::from(f.end_month.max(1)) - 1).copied().unwrap_or(1));
        }
        // No new degrees, so the form cannot fail a lookup. `college_body`'s
        // `?` would be an unreachable error path.
        c.put(&format!("/academicHistory/colleges/{}", f.college_id), &crate::education::college_resave_body(&college))
            .await?;
    }
    Ok(())
}

/// Creates new terms (once per college/term/year) and adds the courses.
pub async fn apply(c: &Client, lk: &Lookups, adds: &[CourseAdd]) -> Result<()> {
    let mut created: BTreeMap<(i64, String, String), i64> = BTreeMap::new();
    for a in adds {
        let term_id = match &a.target {
            TermTarget::Existing { id, .. } => *id,
            TermTarget::New { term, year, in_progress, academic_status } => {
                let k = (a.college_id, term.clone(), year.clone());
                match created.get(&k) {
                    Some(id) => *id,
                    None => {
                        let r = c
                            .post(
                                &format!("/saveOrUpdateTermAndCourses/{}", a.college_id),
                                &json!({ "termId": lk.terms.id(term)?, "yearId": lk.years.id(year)?,
                                         "academicStatusId": lk.academic_status.id(academic_status)?,
                                         "termInProgress": in_progress, "id": null, "courses": [], "year": year }),
                            )
                            .await?;
                        let id = int(&r["id"]);
                        if id == 0 {
                            return Err(ApiError::Shape(format!(
                                "creating {term} {year} at {} returned no id",
                                a.college
                            )));
                        }
                        created.insert(k, id);
                        id
                    }
                }
            }
        };
        let converted = s(&c
            .post("/gradeConversion", &json!({ "grade": a.course.grade, "collegeCode": a.college_code }))
            .await?["grade"]);
        let classification: serde_json::Map<String, Value> = [
            "labSelected",
            "honorsSelected",
            "repeatedSelected",
            "advancedPlacementSelected",
            "otherTestSelected",
            "studyAbroadSelected",
            "labOnlySelected",
            "labAndLectureSelected",
            "dpdProfessionalSelected",
            "dpdScienceSelected",
        ]
        .into_iter()
        .map(|k| (k.to_string(), Value::Bool(false)))
        .collect();
        c.post(
            &format!("/academicHistory/courses/collegeId/{}/termId/{term_id}", a.college_id),
            &json!({ "code": a.course.code, "title": a.course.title, "grade": a.course.grade,
                     "gradeEnabled": !a.course.grade.is_empty(), "convertedGrade": converted,
                     "termId": term_id.to_string(), "subjectId": lk.subjects.id(&a.subject)?.to_string(),
                     "credits": a.course.credits, "source": null, "courseClassification": classification }),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/coursework_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/coursework_regions.rs"]
mod regions;
