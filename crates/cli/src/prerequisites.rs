//! `profile/prerequisites.toml` against the transcripts: each program's
//! rulings (which course counts for which requirement) re-checked whenever
//! the transcripts change, so a dropped course, a failing grade, an
//! ungraded term past its end, or a course too old for a recency limit
//! shows up in `admit audit` instead of in an admissions email.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use admit_domain::{CourseRef, Finding, PrerequisiteProgram, Profile, Requirement};
use admit_sources::TranscriptCourse;
use time::Date;

/// Every transcript in `coursework.toml`, parsed: the courses, and the
/// college labels whose transcript could not be read (with why).
pub struct Transcripts {
    pub courses: Vec<TranscriptCourse>,
    pub unreadable: BTreeMap<String, String>,
}

pub fn read_transcripts(p: &Profile, dir: &Path) -> Transcripts {
    let mut t = Transcripts { courses: vec![], unreadable: BTreeMap::new() };
    for src in &p.coursework.transcripts {
        match admit_liaison::coursework::read(src, dir) {
            Ok(c) => t.courses.extend(c),
            Err(e) => t.unreadable.extend(src.colleges.keys().map(|k| (k.clone(), e.clone()))),
        }
    }
    t
}

/// The findings for the programs reported under `portal` (`None`: those
/// that name no portal).
pub fn findings(p: &Profile, dir: &Path, portal: Option<&str>, today: Date) -> Vec<Finding> {
    let programs: Vec<&PrerequisiteProgram> =
        p.prerequisites.programs.iter().filter(|x| x.portal.as_deref() == portal).collect();
    if programs.is_empty() {
        return vec![];
    }
    let t = read_transcripts(p, dir);
    programs.into_iter().flat_map(|x| check(x, &t, today)).collect()
}

pub fn check(p: &PrerequisiteProgram, t: &Transcripts, today: Date) -> Vec<Finding> {
    let start = match p.start.as_deref() {
        Some(s) => match month(s) {
            Some(m) => Some(m),
            None => return vec![Finding::Violation(format!("{}: start {s:?} is not YYYY-MM", p.name))],
        },
        None if p.requirements.iter().any(|r| r.within_years.is_some()) => {
            return vec![Finding::Violation(format!("{}: within_years needs the program's start (YYYY-MM)", p.name))];
        }
        None => None,
    };
    let min = match p.min_grade.as_deref().map(|g| (g, points(g))) {
        Some((g, None)) => {
            return vec![Finding::Violation(format!("{}: min_grade {g:?} is not a letter grade", p.name))];
        }
        Some((_, Some(x))) => Some(x),
        None => None,
    };
    p.requirements.iter().map(|r| requirement(p, r, t, start, min, today)).collect()
}

fn requirement(
    p: &PrerequisiteProgram,
    r: &Requirement,
    t: &Transcripts,
    start: Option<i32>,
    min: Option<f32>,
    today: Date,
) -> Finding {
    let label = format!("{}: {}", p.name, r.name);
    if let Some(why) = &r.deferred {
        return Finding::Note(format!("{label}: deferred ({why})"));
    }
    if r.courses.is_empty() {
        return Finding::Pending(format!("{label}: no course counted yet"));
    }
    let results: Vec<Status> = r.courses.iter().map(|c| course(c, t, start, r.within_years, min, today)).collect();
    let detail = results.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("; ");
    match worst(results.iter().map(|s| s.level)) {
        Level::Done => Finding::Note(format!("{label}: met by {detail}")),
        Level::Pending => Finding::Pending(format!("{label}: {detail}")),
        Level::Failed => Finding::Violation(format!("{label}: {detail}")),
    }
}

fn worst(levels: impl Iterator<Item = Level>) -> Level {
    levels.max().unwrap_or(Level::Done)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Done,
    Pending,
    Failed,
}

struct Status {
    level: Level,
    text: String,
}

/// The best attempt at `c`: one that counts, else one in progress, else the
/// latest.
fn course(
    c: &CourseRef,
    t: &Transcripts,
    start: Option<i32>,
    within: Option<i32>,
    min: Option<f32>,
    today: Date,
) -> Status {
    let name = format!("{} {}", c.college, c.code);
    let attempts: Vec<&TranscriptCourse> =
        t.courses.iter().filter(|x| x.college.eq_ignore_ascii_case(&c.college) && c.same_code(&x.code)).collect();
    if attempts.is_empty() {
        return match t.unreadable.get(&c.college) {
            Some(e) => Status { level: Level::Pending, text: format!("{name} unchecked: transcript not read ({e})") },
            None => Status { level: Level::Failed, text: format!("{name} is not on any transcript") },
        };
    }
    let mut graded: Vec<Status> = attempts.iter().map(|x| attempt(&name, x, start, within, min, today)).collect();
    graded.sort_by_key(|s| s.level);
    graded.remove(0)
}

fn attempt(
    name: &str,
    x: &TranscriptCourse,
    start: Option<i32>,
    within: Option<i32>,
    min: Option<f32>,
    today: Date,
) -> Status {
    let end = term_end(x);
    let when = format!("{} {}", x.season, x.year);
    let ended = end.map(month_label).unwrap_or_default();
    if x.grade.is_empty() {
        let now = today.year() * 12 + i32::from(u8::from(today.month())) - 1;
        let text = match end {
            Some(e) if e < now => {
                format!("{name} ({when}) ended {ended} but has no grade on the transcript yet (admit laccd fetch)")
            }
            _ => format!("{name} in progress ({when}, ends {ended})"),
        };
        return Status { level: Level::Pending, text };
    }
    let g = x.grade.trim_end_matches('*').to_ascii_uppercase();
    if ["F", "W", "NP", "NC", "I", "U", "WU", "IP"].contains(&g.as_str()) {
        return Status { level: Level::Failed, text: format!("{name} ({when}) graded {} - not completed", x.grade) };
    }
    match (min, points(&g)) {
        (Some(m), Some(p)) if p < m => {
            return Status {
                level: Level::Failed,
                text: format!("{name} ({when}) graded {}, below the minimum", x.grade),
            };
        }
        (Some(_), None) => {
            return Status {
                level: Level::Pending,
                text: format!("{name} ({when}) graded {}: confirm the program takes a non-letter grade", x.grade),
            };
        }
        _ => {}
    }
    if let (Some(n), Some(e), Some(start)) = (within, end, start)
        && e < start - 12 * n
    {
        return Status {
            level: Level::Failed,
            text: format!("{name} ({when}) is older than {n} years before the {} start", month_label(start)),
        };
    }
    Status { level: Level::Done, text: format!("{name} ({}, {when})", x.grade) }
}

/// The term's last month as a month index (year * 12 + month - 1). From
/// the transcript's own dates when it prints them, else by season.
fn term_end(x: &TranscriptCourse) -> Option<i32> {
    let year: i32 = x.year.parse().ok()?;
    let m = match x.term_months {
        Some((_, end)) => end,
        None => match x.season.as_str() {
            "Winter" => 2,
            "Spring" => 6,
            "Summer" => 8,
            "Fall" => 12,
            _ => return None,
        },
    };
    Some(year * 12 + i32::from(m) - 1)
}

fn month(s: &str) -> Option<i32> {
    let (y, m) = s.split_once('-')?;
    let (y, m): (i32, i32) = (y.parse().ok()?, m.parse().ok()?);
    (1..=12).contains(&m).then_some(y * 12 + m - 1)
}

fn month_label(i: i32) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", M[i.rem_euclid(12) as usize], i.div_euclid(12))
}

fn points(g: &str) -> Option<f32> {
    let base = match g.chars().next()? {
        'A' => 4.0,
        'B' => 3.0,
        'C' => 2.0,
        'D' => 1.0,
        _ => return None,
    };
    match &g[1..] {
        "" => Some(base),
        "+" => Some(base + 0.3),
        "-" => Some(base - 0.3),
        _ => None,
    }
}

/// Labels named by a requirement but by no transcript source: a typo there
/// would otherwise read as "not on any transcript".
pub fn unknown_colleges(p: &Profile) -> BTreeSet<String> {
    let known: BTreeSet<&String> = p.coursework.transcripts.iter().flat_map(|t| t.colleges.keys()).collect();
    p.prerequisites
        .programs
        .iter()
        .flat_map(|x| &x.requirements)
        .flat_map(|r| &r.courses)
        .filter(|c| !known.contains(&c.college))
        .map(|c| c.college.clone())
        .collect()
}

#[cfg(test)]
#[path = "tests/prerequisites_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/prerequisites_boundaries.rs"]
mod boundaries;
