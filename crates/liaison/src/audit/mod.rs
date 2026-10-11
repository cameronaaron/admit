//! What still stands between the application and submission, read from the
//! portal: section completion, each program's deadline, recommendations,
//! official transcripts, prerequisites, fees, and whether the attendance
//! dates agree with the coursework entered.
//!
//! Each check is a pure function of the JSON its endpoint returns (pinned by
//! a test with the captured shape); [`run`] only fetches and dispatches.

mod attendance;
mod deadlines;
mod evaluations;
mod fees;
mod navigation;
mod prerequisites;
mod transcripts;

use admit_domain::Finding;
use serde_json::{Value, json};
use time::Date;

use crate::client::{Client, Result, arr, int, s};
use crate::coursework;

pub use deadlines::{Deadline, SOON_DAYS, grade};
pub use evaluations::Recs;
pub use navigation::{from_navigation, from_programs};

/// The selected-programs search the dashboard runs; the only response that
/// carries every selected program's deadline (`/transcriptRequirements`
/// lists only some of them).
pub(crate) async fn selected_program_details(c: &Client, selected: &Value) -> Result<Value> {
    let body = json!({
        "includeAvailablePrograms": true, "includeFuturePrograms": true, "includePastPrograms": true,
        "querySelectedPrograms": true, "extendedProfileAnswerIds": [],
        "casLevelUnofficialUploadsReceived": selected["casLevelUnofficialUploadsReceived"],
        "unofficialTranscriptUploadsRequiredForSubmission": selected["unofficialTranscriptUploadsRequiredForSubmission"],
        "transcriptTypeAccepted": selected["transcriptTypeAccepted"],
        "featureSwitches": ["CONFIG_PROGRAM_AVAILABILITY", "MOBILE_UX_SWITCH"],
        "filters": {}, "preLaunchEnvironment": false, "query": "",
        "selectedPrograms": selected["selectedPrograms"],
    });
    c.query("/search/filteredQueries", &body).await
}

/// Every unsubmitted selected program's deadline.
pub async fn deadlines(c: &Client) -> Result<Vec<Deadline>> {
    let selected = c.get("/programsSelected").await?;
    Ok(deadlines::read(&selected_program_details(c, &selected).await?["programs"]))
}

/// Each selected program's recommendations, with its deadline when it is
/// not yet submitted (joined on `programSelectedId`).
pub async fn recommendations(c: &Client) -> Result<Vec<Recs>> {
    let selected = c.get("/programsSelected").await?;
    let due = deadlines::by_program(&selected_program_details(c, &selected).await?["programs"]);
    let mut out = Vec::new();
    for p in arr(&c.get("/getProgramNavigation").await?) {
        let id = int(&p["programSelectedId"]);
        let label = format!("{} - {}", s(&p["orgName"]), s(&p["programName"]));
        let ev = c.get(&format!("/programMaterials/evaluations/programSelected/{id}")).await?;
        out.extend(evaluations::read(&label, &ev, due.get(&id).copied()));
    }
    Ok(out)
}

/// `shared`: institutions that share one transcript (from `coursework.toml`).
pub async fn run(c: &Client, today: Date, shared: &[Vec<String>]) -> Result<Vec<Finding>> {
    let mut out = from_navigation(&c.get("/navigation").await?);
    let programs = c.get("/getProgramNavigation").await?;
    out.extend(from_programs(&programs));

    let selected = c.get("/programsSelected").await?;
    let details = selected_program_details(c, &selected).await?;
    out.extend(deadlines::check(&details["programs"], today));
    out.extend(fees::check(&selected["selectedPrograms"]));

    for p in arr(&programs) {
        let label = format!("{} - {}", s(&p["orgName"]), s(&p["programName"]));
        let ev =
            c.get(&format!("/programMaterials/evaluations/programSelected/{}", int(&p["programSelectedId"]))).await?;
        out.extend(evaluations::check(&label, &ev));
    }
    out.extend(transcripts::check(&c.get("/transcripts").await?["college"], shared));
    out.extend(prerequisites::check(&c.get("/academicHistory/courseWork/prerequisites").await?));

    out.extend(attendance::check(&coursework::live(c).await?));
    Ok(out)
}
