//! What `apply` would do ([`Plan`]), per application [`Section`], and how it
//! prints.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use admit_domain::{Achievement, Experience, Finding, License};
use serde_json::Value;

use crate::coursework::{self, CourseAdd, TermTarget};
use crate::education::CollegeUpdate;
use crate::reconcile::{Diff, Step};
use crate::{client, documents, high_school, prereqs, questions, references};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Achievements,
    Licenses,
    Experiences,
    Education,
    Coursework,
    HighSchool,
    Prerequisites,
    Questions,
    Documents,
    References,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::Achievements,
        Section::Licenses,
        Section::Experiences,
        Section::Education,
        Section::Coursework,
        Section::HighSchool,
        Section::Prerequisites,
        Section::Questions,
        Section::Documents,
        Section::References,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Section::Achievements => "achievements",
            Section::Licenses => "licenses",
            Section::Experiences => "experiences",
            Section::Education => "education",
            Section::Coursework => "coursework",
            Section::HighSchool => "high-school",
            Section::Prerequisites => "prerequisites",
            Section::Questions => "questions",
            Section::Documents => "documents",
            Section::References => "references",
        }
    }

    pub fn parse(s: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|x| x.name() == s)
    }
}

/// Everything `apply` would do, plus what it can't.
#[derive(Debug, Default)]
pub struct Plan {
    pub achievements: Option<Diff<Achievement>>,
    pub licenses: Option<Diff<License>>,
    pub experiences: Option<Diff<Experience>>,
    /// Live question blocks per experience id, needed to edit in place.
    pub experience_blocks: BTreeMap<i64, BTreeMap<String, Value>>,
    pub colleges: Vec<CollegeUpdate>,
    pub courses: Vec<CourseAdd>,
    /// Mistyped portal courses to correct from the transcript.
    pub course_edits: Vec<coursework::CourseEdit>,
    /// "Still attending" flags the transcript contradicts.
    pub attendance: Vec<coursework::AttendanceFix>,
    /// Institutions to add before their coursework can be entered.
    pub new_colleges: Vec<coursework::CollegeCreate>,
    pub high_school: Vec<high_school::Change>,
    /// Courses to assign to program prerequisites.
    pub prereqs: Vec<prereqs::Assign>,
    /// Program question answers to save, one save per program.
    pub questions: Vec<questions::Save>,
    /// Program documents to upload, one save per program.
    pub documents: Vec<documents::Save>,
    /// Recommendation letters to request (each emails the recommender).
    pub references: Vec<references::Request>,
    pub findings: Vec<Finding>,
}

impl Plan {
    pub fn change_count(&self) -> usize {
        self.achievements.as_ref().map_or(0, |d| d.steps.len())
            + self.licenses.as_ref().map_or(0, |d| d.steps.len())
            + self.experiences.as_ref().map_or(0, |d| d.steps.len())
            + self.colleges.len()
            + self.courses.len()
            + self.course_edits.len()
            + self.attendance.len()
            + self.new_colleges.len()
            + self.high_school.len()
            + self.prereqs.len()
            + self.questions.iter().map(|q| q.changes.len()).sum::<usize>()
            + self.documents.iter().map(|d| d.uploads.len() + usize::from(d.complete)).sum::<usize>()
            + self.references.len()
    }

    /// The sections this plan writes to.
    pub fn sections(&self) -> std::collections::BTreeSet<Section> {
        let written = [
            (Section::Achievements, self.achievements.as_ref().is_some_and(|d| !d.steps.is_empty())),
            (Section::Licenses, self.licenses.as_ref().is_some_and(|d| !d.steps.is_empty())),
            (Section::Experiences, self.experiences.as_ref().is_some_and(|d| !d.steps.is_empty())),
            (Section::Education, !self.colleges.is_empty()),
            (
                Section::Coursework,
                !(self.courses.is_empty()
                    && self.course_edits.is_empty()
                    && self.attendance.is_empty()
                    && self.new_colleges.is_empty()),
            ),
            (Section::HighSchool, !self.high_school.is_empty()),
            (Section::Prerequisites, !self.prereqs.is_empty()),
            (Section::Questions, self.questions.iter().any(|q| !q.changes.is_empty())),
            (Section::Documents, !self.documents.is_empty()),
            (Section::References, !self.references.is_empty()),
        ];
        written.into_iter().filter(|(_, on)| *on).map(|(s, _)| s).collect()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        fn section<T>(out: &mut String, name: &str, d: &Option<Diff<T>>, label: impl Fn(&T) -> String) {
            let Some(d) = d else { return };
            let _ = writeln!(out, "{name}: {} change(s)", d.steps.len());
            for st in &d.steps {
                let _ = match st {
                    Step::Create(x) => writeln!(out, "  + {}", label(x)),
                    Step::Update { desired, fields, .. } => {
                        writeln!(out, "  ~ {} ({})", label(desired), fields.join(", "))
                    }
                };
            }
            for e in &d.extras {
                let _ = writeln!(out, "  = {e} (on the portal, not in the profile: left alone)");
            }
        }
        section(&mut out, "achievements", &self.achievements, |a| format!("{:?} {:?}", a.kind, a.name));
        section(&mut out, "licenses", &self.licenses, |l| format!("{:?}", l.title));
        section(&mut out, "experiences", &self.experiences, Experience::key);
        if !self.colleges.is_empty() {
            let _ = writeln!(out, "education: {} college(s) to update", self.colleges.len());
            for u in &self.colleges {
                let _ = writeln!(out, "  {}", client::s(&u.college["college"]));
                for c in &u.changes {
                    let _ = writeln!(out, "    {c}");
                }
            }
        }
        if !self.new_colleges.is_empty() {
            let _ = writeln!(
                out,
                "colleges: {} to add (their coursework follows on the next pass)",
                self.new_colleges.len()
            );
            for c in &self.new_colleges {
                let _ = writeln!(
                    out,
                    "  + {} | {} {} – {} | months {}-{}",
                    c.name,
                    c.first.0,
                    c.first.1,
                    if c.still_attending {
                        "still attending".to_string()
                    } else {
                        format!("{} {}", c.last.0, c.last.1)
                    },
                    c.months.0,
                    c.months.1
                );
            }
        }
        if !self.high_school.is_empty() {
            let _ = writeln!(out, "high school: {} change(s)", self.high_school.len());
            for c in &self.high_school {
                let _ = writeln!(out, "  {}", c.describe());
            }
        }
        if !self.courses.is_empty() {
            let _ = writeln!(out, "coursework: {} course(s) to add", self.courses.len());
            for a in &self.courses {
                let term = match &a.target {
                    TermTarget::Existing { label, .. } => label.clone(),
                    TermTarget::New { term, year, in_progress, .. } => {
                        format!("NEW {term} {year}{}", if *in_progress { " (in progress)" } else { "" })
                    }
                };
                let _ = writeln!(
                    out,
                    "  + {} | {term} | {} {} | {} cr | grade {} | {}",
                    a.college,
                    a.course.code,
                    a.course.title,
                    a.course.credits,
                    if a.course.grade.is_empty() { "-" } else { &a.course.grade },
                    a.subject
                );
            }
        }
        if !self.course_edits.is_empty() {
            let _ = writeln!(out, "coursework: {} course(s) to correct", self.course_edits.len());
            for e in &self.course_edits {
                let mut what = Vec::new();
                if e.from != e.to {
                    what.push(format!("{} {:?} -> {} {:?}", e.from.0, e.from.1, e.to.0, e.to.1));
                }
                if let Some((was, now)) = &e.subject {
                    what.push(format!("subject {was} -> {now}"));
                }
                let _ = writeln!(out, "  ~ {} | {} | {} | {}", e.college, e.term, e.from.0, what.join("; "));
            }
        }
        if !self.attendance.is_empty() {
            let _ = writeln!(out, "attendance: {} college(s) to update", self.attendance.len());
            for a in &self.attendance {
                let now = if a.still_attending {
                    "still attending".to_string()
                } else {
                    format!("ended {} {} (month {})", a.last.0, a.last.1, a.end_month)
                };
                let _ = writeln!(out, "  ~ {} | {} -> {now}", a.college, a.was);
            }
        }
        if !self.prereqs.is_empty() {
            let _ = writeln!(out, "prerequisites: {} course(s) to assign", self.prereqs.len());
            for a in &self.prereqs {
                let _ = writeln!(out, "  + {a}");
            }
        }
        for q in &self.questions {
            let _ = writeln!(out, "questions: {} answer(s) for {}", q.changes.len(), q.program);
            for c in &q.changes {
                let from = c.from.as_deref().unwrap_or("(blank)");
                let _ = writeln!(
                    out,
                    "  ~ {} {:?}: {from:?} -> {:?}",
                    c.question_id,
                    c.question.chars().take(60).collect::<String>(),
                    c.to
                );
            }
        }
        for d in &self.documents {
            let _ = writeln!(out, "documents: {} file(s) for {}", d.uploads.len(), d.program);
            for u in &d.uploads {
                let was = match &u.replaces {
                    Some(h) if u.changed => format!(" (replaces {:?}: the portal's copy differs)", h.name),
                    Some(h) => format!(" (replaces {:?})", h.name),
                    None => String::new(),
                };
                let _ = writeln!(out, "  + {} {}: {}{was}", u.document_id, u.kind, u.path.display());
            }
            if d.complete {
                let _ = writeln!(out, "  = mark Documents done");
            }
        }
        if !self.references.is_empty() {
            let _ = writeln!(out, "references: {} request(s), each emails the recommender", self.references.len());
            for q in &self.references {
                let r = &q.recommender;
                let _ = writeln!(
                    out,
                    "  + {} {} <{}> for {} (due {}, waived: {})",
                    r.first_name, r.last_name, r.email, q.program, r.due, r.waive
                );
            }
        }
        for f in &self.findings {
            let _ = writeln!(out, "{f}");
        }
        out
    }
}
