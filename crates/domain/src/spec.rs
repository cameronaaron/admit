//! Profile files. Each `profile/<section>.toml` deserializes into one of the
//! `*File` types; unknown keys are rejected so a typo can't silently drop data.
//!
//! Entries match live portal records by a natural key (documented per type).
//! `ids` maps a portal name to that portal's record id; `admit pull` fills it
//! so an entry can be renamed without being re-created.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub type PortalIds = BTreeMap<String, i64>;

fn is_empty_ids(ids: &PortalIds) -> bool {
    ids.is_empty()
}

fn default_country() -> String {
    "United States".into()
}

fn is_us(c: &String) -> bool {
    c == "United States"
}

// ---------------------------------------------------------------- achievements

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AchievementKind {
    Award,
    Honor,
}

/// Natural key: `name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Achievement {
    pub kind: AchievementKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<String>,
    /// `YYYY-MM-DD`; omit when the source gives no date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub state: String,
    #[serde(default = "default_country", skip_serializing_if = "is_us")]
    pub country: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "is_empty_ids")]
    pub ids: PortalIds,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AchievementsFile {
    #[serde(default, rename = "achievement")]
    pub achievements: Vec<Achievement>,
}

// -------------------------------------------------------------------- licenses

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseKind {
    Certification,
    Licensure,
}

/// Natural key: `title`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    pub kind: LicenseKind,
    pub title: String,
    pub organization: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `YYYY-MM-DD`; omit for credentials that don't expire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "is_empty_ids")]
    pub ids: PortalIds,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicensesFile {
    #[serde(default, rename = "license")]
    pub licenses: Vec<License>,
}

// ----------------------------------------------------------------- experiences

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExperienceKind {
    Employment,
    PatientCare,
    Community,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Supervisor {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub phone: String,
}

impl Supervisor {
    pub fn is_empty(&self) -> bool {
        *self == Supervisor::default()
    }
}

/// Natural key: `organization` + `title` + `start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Experience {
    pub kind: ExperienceKind,
    pub organization: String,
    pub title: String,
    pub state: String,
    #[serde(default = "default_country", skip_serializing_if = "is_us")]
    pub country: String,
    /// `YYYY-MM-DD`.
    pub start: String,
    /// `YYYY-MM-DD`; omit while `current`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default)]
    pub current: bool,
    /// Portal wording, e.g. "Full time", "Part time", "Temporary".
    pub status: String,
    pub hours_per_week: u32,
    pub weeks: u32,
    pub total_hours: u32,
    pub compensated: bool,
    pub volunteer: bool,
    pub description: String,
    #[serde(default, skip_serializing_if = "Supervisor::is_empty")]
    pub supervisor: Supervisor,
    #[serde(default, skip_serializing_if = "is_empty_ids")]
    pub ids: PortalIds,
}

impl Experience {
    pub fn key(&self) -> String {
        format!("{} / {} / {}", self.organization, self.title, self.start)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperiencesFile {
    #[serde(default, rename = "experience")]
    pub experiences: Vec<Experience>,
}

// ------------------------------------------------------------------- education

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DegreeStatus {
    Awarded,
    Expected,
}

/// Natural key within its college: `kind` + `major` + `second_major`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Degree {
    /// Portal degree type, e.g. "Associate of Arts", "Certificate".
    pub kind: String,
    pub major: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_major: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minor: Option<String>,
    pub status: DegreeStatus,
    /// Month name, e.g. "August".
    pub month: String,
    pub year: i32,
    /// Free text kept for people (e.g. the transcript's exact title); not sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Degree {
    pub fn key(&self) -> String {
        match &self.second_major {
            Some(s) => format!("{} / {} + {}", self.kind, self.major, s),
            None => format!("{} / {}", self.kind, self.major),
        }
    }
}

/// Natural key: `name` (the portal's institution name). Colleges are matched,
/// never created: adding one starts a transcript requirement, so do it by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct College {
    pub name: String,
    #[serde(default, rename = "degree", skip_serializing_if = "Vec::is_empty")]
    pub degrees: Vec<Degree>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EducationFile {
    #[serde(default, rename = "college")]
    pub colleges: Vec<College>,
}

// ------------------------------------------------------------------ coursework

/// Where coursework comes from: transcripts parsed by `admit-sources`, plus
/// the human decisions a transcript can't answer (subjects, summer session).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseworkFile {
    #[serde(default, rename = "transcript")]
    pub transcripts: Vec<TranscriptSource>,
}

impl CourseworkFile {
    /// Every transcript's college label → portal institution name.
    pub fn colleges(&self) -> std::collections::BTreeMap<String, String> {
        self.transcripts.iter().flat_map(|t| t.colleges.clone()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSource {
    /// Path to the transcript, relative to the profile directory or absolute.
    pub path: String,
    /// Parser name; `laccd` is the only one so far.
    pub parser: String,
    /// Transcript college abbreviation → portal institution name.
    pub colleges: BTreeMap<String, String>,
    /// Portal term used for the transcript's "Summer" (e.g. "Summer 2").
    pub summer_term: String,
    /// Portal academic status for terms this tool creates.
    pub academic_status: String,
    /// Course codes to leave out.
    #[serde(default)]
    pub skip: Vec<String>,
    /// Full course code, or department prefix, → portal subject name.
    #[serde(default)]
    pub subjects: BTreeMap<String, String>,
    /// Course codes whose portal title is replaced by the transcript's (for
    /// typos; other titles are left alone, since the portal's are often
    /// fuller than the transcript's abbreviations).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fix_titles: Vec<String>,
    /// Add an institution to the portal when the transcript shows coursework
    /// there and the portal doesn't list it. Off by default: a new
    /// institution starts a transcript requirement on the portal.
    #[serde(default)]
    pub create_missing_colleges: bool,
}

// ----------------------------------------------------------------- high school

/// One high school; matched by `name` (the portal's school name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighSchool {
    pub name: String,
    /// `YYYY-MM` attended from.
    pub from: String,
    /// `YYYY-MM` attended to.
    pub to: String,
    /// Portal wording, e.g. "High School Diploma".
    pub diploma: String,
    /// `YYYY-MM` the diploma was awarded.
    pub diploma_date: String,
    /// The school's calendar: "Semester", "Quarter" or "Trimester". Required
    /// by the portal's attendance form when it has none stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighSchoolFile {
    #[serde(default, rename = "high_school")]
    pub high_schools: Vec<HighSchool>,
}

// -------------------------------------------------------------------- personal

/// Answers about the applicant that most applications ask and no other
/// profile file holds (demographics, family education, residency, fee
/// waiver). Stated by the applicant; portals' answer files are filled from
/// here. Every field is optional: absent means "not stated yet - ask".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Personal {
    pub legal_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    /// YYYY-MM-DD.
    pub birthdate: Option<String>,
    pub birth_city: Option<String>,
    pub birth_county: Option<String>,
    pub birth_state: Option<String>,
    pub birth_country: Option<String>,
    pub citizenship: Option<String>,
    pub gender_identity: Option<String>,
    pub sex: Option<String>,
    pub pronouns: Option<String>,
    pub sexual_orientation: Option<String>,
    pub native_language: Option<String>,
    pub california_resident: Option<bool>,
    /// "none" when not affiliated with the armed forces.
    pub military: Option<String>,
    pub foster_youth: Option<bool>,
    pub hispanic: Option<bool>,
    pub race: Option<Vec<String>>,
    pub parent1_education: Option<String>,
    pub parent2_education: Option<String>,
    pub has_siblings: Option<bool>,
    pub siblings_attended_college: Option<bool>,
    pub siblings_graduate_degree: Option<bool>,
    pub receiving_financial_aid: Option<bool>,
    pub applying_financial_aid: Option<bool>,
    #[serde(default)]
    pub address: Vec<PostalAddress>,
    pub fee_waiver: Option<FeeWaiver>,
    /// Where each answer came from, for the record.
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostalAddress {
    /// `mailing`, `permanent`, or both as two entries.
    pub kind: String,
    pub street: String,
    pub city: String,
    pub region: String,
    pub country: String,
    pub postal: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeeWaiver {
    /// `income` (household AGI under the limit), `benefits`, or
    /// `enrolled-need` (enrolled, need-based aid, aid officer letter).
    pub basis: String,
    pub household_size: Option<u32>,
    /// Enrolled in a college now (as of `as_of`).
    pub enrolled: Option<bool>,
    pub as_of: Option<String>,
}

// --------------------------------------------------------------- prerequisites

/// `prerequisites.toml`: what each program has ruled counts for its
/// prerequisites, checked against the parsed transcripts by `admit audit`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrerequisitesFile {
    #[serde(default, rename = "program")]
    pub programs: Vec<PrerequisiteProgram>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrerequisiteProgram {
    pub name: String,
    /// The `[[portal]]` whose audit reports it; none: the profile's.
    pub portal: Option<String>,
    /// First month of the program, YYYY-MM: recency limits count back from
    /// it, so a program with `within_years` needs one. A portal states none.
    pub start: Option<String>,
    /// Lowest grade that counts (`C`); unset: any passing grade.
    pub min_grade: Option<String>,
    /// Where the rulings come from (a note, an email thread).
    pub source: Option<String>,
    #[serde(rename = "requirement")]
    pub requirements: Vec<Requirement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub name: String,
    /// Every listed course is needed (two for "two writing courses").
    #[serde(default)]
    pub courses: Vec<CourseRef>,
    /// Must be completed within this many years before `start`.
    pub within_years: Option<i32>,
    /// Not needed before admission, and why ("may be completed at UCLA").
    pub deferred: Option<String>,
}

/// A course as its transcript prints it: the college label (`LAVC`,
/// `Connecticut College`) and the code (`STAT 101`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourseRef {
    pub college: String,
    pub code: String,
}

impl CourseRef {
    /// This course's code against another, ignoring spaces and leading zeros
    /// in the number: a note's "PSYCH 41" is the transcript's "PSYCH 041",
    /// "FAM & CS 21" its "FAM &CS 021".
    pub fn same_code(&self, other: &str) -> bool {
        fn norm(s: &str) -> String {
            let mut out = String::new();
            let mut lead = true;
            for ch in s.chars().filter(|c| !c.is_whitespace()) {
                if ch.is_ascii_digit() {
                    if lead && ch == '0' {
                        continue;
                    }
                    lead = false;
                } else {
                    lead = true;
                }
                out.push(ch.to_ascii_uppercase());
            }
            out
        }
        norm(&self.code) == norm(other)
    }
}

/// `questions.toml`: each program's answers to its own questions on a
/// portal, keyed by the portal's question id. A choice is written as its
/// option's text; free text as itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionsFile {
    #[serde(default, rename = "program")]
    pub programs: Vec<ProgramAnswers>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramAnswers {
    /// "Organization - Program" as the portal shows it.
    pub name: String,
    pub portal: String,
    #[serde(default)]
    pub answers: std::collections::BTreeMap<String, String>,
}

/// `documents.toml`: the file each program's document slot should hold,
/// keyed by the portal's document id (`admit documents` lists them). Paths
/// are relative to the profile directory, or absolute.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentsFile {
    #[serde(default, rename = "program")]
    pub programs: Vec<ProgramDocuments>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDocuments {
    /// "Organization - Program" as the portal shows it.
    pub name: String,
    pub portal: String,
    #[serde(default)]
    pub files: std::collections::BTreeMap<String, String>,
}

/// `references.toml`: the recommenders to request letters from, and for
/// which programs. Requesting one emails the recommender.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferencesFile {
    /// The applicant agrees, for every request, to both statements the form
    /// requires: permission to contact the reference by email, and for the
    /// schools to contact them. Nothing is requested unless this is true.
    #[serde(default)]
    pub consent: bool,
    #[serde(default, rename = "recommender")]
    pub recommenders: Vec<Recommender>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recommender {
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    /// Requested due date, `YYYY-MM-DD`.
    pub due: String,
    /// "I waive my rights of access to this evaluation".
    pub waive: bool,
    /// Personal message to the evaluator (the form allows 500 characters).
    pub message: String,
    /// "Organization - Program" names, as the portal shows them.
    pub programs: Vec<String>,
    /// The evaluation type, when a program has more than one.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
}

/// `transcript_orders.toml`: official transcripts that must reach a school,
/// each with the ordering service that sends it (`admit transcripts`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptOrdersFile {
    /// The email the Clearinghouse orders were placed with: it has no
    /// accounts, and finds an order only by its number and that email.
    #[serde(default)]
    pub email: String,
    #[serde(default, rename = "send")]
    pub sends: Vec<TranscriptSend>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptSend {
    /// The school whose transcript it is, as the service names it.
    pub from: String,
    /// The recipient, as the service names it.
    pub to: String,
    /// `clearinghouse` or `parchment`.
    pub via: String,
    /// The service's number for the order: required to track a
    /// Clearinghouse order; for Parchment the delivery id (`TR…`), optional,
    /// since the account lists its orders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
}

// --------------------------------------------------------------------- profile

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub achievements: AchievementsFile,
    pub licenses: LicensesFile,
    pub experiences: ExperiencesFile,
    pub education: EducationFile,
    pub coursework: CourseworkFile,
    pub high_school: HighSchoolFile,
    pub personal: Personal,
    pub prerequisites: PrerequisitesFile,
    pub questions: QuestionsFile,
    pub documents: DocumentsFile,
    pub references: ReferencesFile,
    pub transcript_orders: TranscriptOrdersFile,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("reading {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("parsing {path}: {source}")]
    Toml { path: String, source: toml::de::Error },
}

fn load<T: for<'de> Deserialize<'de> + Default>(dir: &Path, name: &str) -> Result<T, LoadError> {
    let path = dir.join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            toml::from_str(&text).map_err(|source| LoadError::Toml { path: path.display().to_string(), source })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(source) => Err(LoadError::Io { path: path.display().to_string(), source }),
    }
}

impl Profile {
    /// Loads every section file in `dir`; a missing file is an empty section.
    pub fn load(dir: &Path) -> Result<Profile, LoadError> {
        Ok(Profile {
            achievements: load(dir, "achievements.toml")?,
            licenses: load(dir, "licenses.toml")?,
            experiences: load(dir, "experiences.toml")?,
            education: load(dir, "education.toml")?,
            coursework: load(dir, "coursework.toml")?,
            high_school: load(dir, "high_school.toml")?,
            personal: load(dir, "personal.toml")?,
            prerequisites: load(dir, "prerequisites.toml")?,
            questions: load(dir, "questions.toml")?,
            documents: load(dir, "documents.toml")?,
            references: load(dir, "references.toml")?,
            transcript_orders: load(dir, "transcript_orders.toml")?,
        })
    }
}
