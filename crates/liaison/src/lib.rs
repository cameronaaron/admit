//! Liaison CAS adapter (NursingCAS, and the other CASs on the same platform).
//!
//! Every Liaison CAS serves the same applicant API under
//! `https://<cas><cycle>.cas.myliaison.com/applicant/api`; only the instance
//! URL differs. Request shapes here were captured from the applicant web app
//! (HARs, 2026-10) and its JS bundle; each module says which calls it uses.
//!
//! [`Portal::plan`] diffs a profile against the live application;
//! [`Portal::apply`] executes a plan. Nothing outside the profile is deleted.

pub mod audit;
pub mod client;
pub mod drift;
pub mod lookup;
pub mod personal;
mod plan;
pub mod reconcile;
pub mod sections;

pub use sections::{
    achievements, coursework, documents, education, experiences, high_school, licenses, prereqs, questions, references,
};

use std::collections::BTreeSet;
use std::path::Path;

use admit_domain::{
    Achievement, AchievementsFile, EducationFile, Experience, ExperiencesFile, Finding, License, LicensesFile, Profile,
};

pub use client::{ApiError, Client, Result};
use lookup::Lookups;
use reconcile::{Step, diff};

pub use plan::{Plan, Section};

pub struct Portal {
    /// Key in each profile entry's `ids` map.
    pub name: String,
    pub client: Client,
}

impl Portal {
    pub async fn lookups(&self) -> Result<Lookups> {
        Lookups::fetch(&self.client).await
    }

    pub async fn plan(&self, p: &Profile, profile_dir: &Path, only: &BTreeSet<Section>, lk: &Lookups) -> Result<Plan> {
        let c = &self.client;
        let mut plan = Plan::default();
        let on = |s: Section| only.is_empty() || only.contains(&s);
        let name = self.name.as_str();
        if on(Section::Achievements) {
            let live = achievements::live(c, lk).await?;
            plan.achievements = Some(diff(name, &p.achievements.achievements, &live, |a| a.name.clone(), |a| &a.ids));
        }
        if on(Section::Licenses) {
            let live = licenses::live(c).await?;
            plan.licenses = Some(diff(name, &p.licenses.licenses, &live, |l| l.title.clone(), |l| &l.ids));
        }
        if on(Section::Experiences) {
            let live = experiences::live(c, lk).await?;
            let pairs: Vec<(i64, Experience)> = live.iter().map(|(i, x, _)| (*i, x.clone())).collect();
            plan.experiences = Some(diff(name, &p.experiences.experiences, &pairs, Experience::key, |x| &x.ids));
            plan.experience_blocks = live.into_iter().map(|(i, _, b)| (i, b)).collect();
        }
        if on(Section::Education) {
            let (updates, findings) = education::plan(&p.education.colleges, &education::live(c).await?);
            plan.colleges = updates;
            plan.findings.extend(findings);
        }
        if on(Section::Coursework) && !p.coursework.transcripts.is_empty() {
            let live = coursework::live(c).await?;
            for src in &p.coursework.transcripts {
                match coursework::read(src, profile_dir) {
                    Ok(courses) => {
                        let cp = coursework::plan(src, &courses, &live, |s| lk.subjects.id(s).ok());
                        plan.courses.extend(cp.adds);
                        plan.course_edits.extend(cp.edits);
                        plan.attendance.extend(cp.attendance);
                        plan.new_colleges.extend(cp.colleges);
                        plan.findings.extend(cp.findings);
                    }
                    Err(e) => plan.findings.push(Finding::Violation(e)),
                }
            }
        }
        if on(Section::HighSchool) && !p.high_school.high_schools.is_empty() {
            let live = high_school::live(c).await?;
            let (changes, findings) = high_school::plan(&p.high_school.high_schools, &live, lk);
            plan.high_school = changes;
            plan.findings.extend(findings);
        }
        if on(Section::Prerequisites) {
            let (live, pool) = prereqs::live(c).await?;
            let (assigns, findings) =
                prereqs::plan(&p.prerequisites.programs, name, &p.coursework.colleges(), &live, &pool);
            plan.prereqs = assigns;
            plan.findings.extend(findings);
        }
        if on(Section::Questions) {
            let (saves, findings) = questions::plan(&p.questions.programs, name, &questions::live(c).await?);
            plan.questions = saves;
            plan.findings.extend(findings);
        }
        if on(Section::Documents) {
            let live = documents::live(c).await?;
            let held = documents::held(c, &p.documents.programs, name, &live).await?;
            let (saves, findings) = documents::plan(&p.documents.programs, name, &live, &held, profile_dir);
            plan.documents = saves;
            plan.findings.extend(findings);
        }
        if on(Section::References) && !p.references.recommenders.is_empty() {
            let (requests, findings) = references::plan(&p.references, &references::live(c).await?);
            plan.references = requests;
            plan.findings.extend(findings);
        }
        Ok(plan)
    }

    pub async fn apply(&self, plan: &Plan, lk: &Lookups) -> Result<()> {
        let c = &self.client;
        for st in plan.achievements.iter().flat_map(|d| &d.steps) {
            match st {
                Step::Create(a) => achievements::create(c, lk, a).await?,
                Step::Update { id, desired, .. } => achievements::update(c, lk, *id, desired).await?,
            }
        }
        for st in plan.licenses.iter().flat_map(|d| &d.steps) {
            match st {
                Step::Create(l) => licenses::save(c, lk, None, l).await?,
                Step::Update { id, desired, .. } => licenses::save(c, lk, Some(*id), desired).await?,
            }
        }
        for st in plan.experiences.iter().flat_map(|d| &d.steps) {
            match st {
                Step::Create(x) => experiences::create(c, lk, x).await?,
                Step::Update { id, desired, .. } => {
                    let blocks = plan.experience_blocks.get(id).cloned().unwrap_or_default();
                    experiences::update(c, lk, *id, desired, &blocks).await?
                }
            }
        }
        for u in &plan.colleges {
            education::apply(c, lk, u).await?;
        }
        for h in &plan.high_school {
            high_school::apply(c, lk, h).await?;
        }
        for nc in &plan.new_colleges {
            coursework::create_college(c, lk, nc).await?;
        }
        coursework::apply(c, lk, &plan.courses).await?;
        coursework::apply_edits(c, &plan.course_edits).await?;
        // After the courses: a course is assignable only once entered.
        prereqs::apply(c, &plan.prereqs).await?;
        questions::apply(c, &plan.questions).await?;
        documents::apply(c, &plan.documents).await?;
        references::apply(c, &plan.references).await?;
        coursework::apply_attendance(c, lk, &plan.attendance).await
    }

    /// The live application as profile sections, with this portal's ids filled
    /// in. Coursework is transcript-driven and not pulled.
    pub async fn pull(&self, lk: &Lookups) -> Result<Profile> {
        let c = &self.client;
        let tag = |id: i64| [(self.name.clone(), id)].into_iter().collect();
        Ok(Profile {
            achievements: AchievementsFile {
                achievements: achievements::live(c, lk)
                    .await?
                    .into_iter()
                    .map(|(id, a)| Achievement { ids: tag(id), ..a })
                    .collect(),
            },
            licenses: LicensesFile {
                licenses: licenses::live(c).await?.into_iter().map(|(id, l)| License { ids: tag(id), ..l }).collect(),
            },
            experiences: ExperiencesFile {
                experiences: experiences::live(c, lk)
                    .await?
                    .into_iter()
                    .map(|(id, x, _)| Experience { ids: tag(id), ..x })
                    .collect(),
            },
            education: EducationFile { colleges: education::pull(&education::live(c).await?) },
            coursework: Default::default(),
            high_school: admit_domain::HighSchoolFile {
                high_schools: high_school::pull(&high_school::live(c).await?, lk),
            },
            // Not held by NursingCAS in a form this reads; kept in personal.toml.
            personal: Default::default(),
            // A program's rulings, not portal data.
            prerequisites: Default::default(),
            questions: admit_domain::QuestionsFile {
                programs: questions::pull(&questions::live(c).await?, &self.name),
            },
            // Local files; the portal holds only their names.
            documents: Default::default(),
            // Who to ask and the applicant's consent: not portal data.
            references: Default::default(),
            // Placed with the ordering services, not on the portal.
            transcript_orders: Default::default(),
        })
    }

    pub async fn audit(&self, p: &Profile, today: time::Date) -> Result<Vec<Finding>> {
        // Each transcript source mapping several labels to institutions (the
        // LACCD district transcript) is one transcript shared by them.
        let shared: Vec<Vec<String>> = p
            .coursework
            .transcripts
            .iter()
            .map(|t| t.colleges.values().cloned().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>())
            .filter(|g| g.len() > 1)
            .collect();
        audit::run(&self.client, today, &shared).await
    }

    pub async fn deadlines(&self) -> Result<Vec<audit::Deadline>> {
        audit::deadlines(&self.client).await
    }
}
