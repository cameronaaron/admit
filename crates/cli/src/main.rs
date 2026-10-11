//! `admit`: keep school applications in line with a declared profile.
//!
//!   admit validate            check the profile offline
//!   admit plan                what `apply` would change on the portal
//!   admit apply --yes         make those changes, then confirm convergence
//!   admit pull [--force]      write the live application into profile/*.toml
//!   admit audit               every portal: deadlines soonest first, what's open
//!   admit -p ucla audit       one portal in full, notes included
//!   admit website             profile vs the applicant's own website
//!   admit transcript FILE     show how a transcript parses
//!   admit session set|check   store / test the portal cookie (Keychain)
//!   admit -p ucla plan        the same commands on a Slate portal (answers file)
//!   admit -p apu status       a submitted Slate application: checklists, latest letter
//!   admit laccd login|fetch   store the LACCD sign-in / download the transcript
//!   admit transcripts         each official transcript order's status (Clearinghouse, Parchment)
//!   admit parchment login     store the Parchment sign-in (asks for the emailed code)
//!   admit sync                unattended: fetch (or newest inbox) transcript -> portal
//!   admit schedule install    run `sync` from launchd on the configured dates

use std::path::PathBuf;

mod commands;
mod ctx;
mod prerequisites;
mod session;
mod sync;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ctx::Ctx;

#[derive(Parser)]
#[command(name = "admit", about = "Declarative school-application automation")]
struct Cli {
    /// Config file.
    #[arg(short, long, default_value = "admit.toml")]
    config: PathBuf,
    /// Portal to act on (default: the first in the config).
    #[arg(short, long)]
    portal: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check the profile offline (field limits, dates, overlapping hours).
    Validate,
    /// Show what `apply` would change.
    Plan {
        /// Comma-separated sections: achievements,licenses,experiences,education,coursework,high-school,prerequisites,questions.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
    /// Apply the plan, then re-plan and fail unless nothing is left to do.
    Apply {
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Required: confirms you reviewed the plan printed just above.
        #[arg(long)]
        yes: bool,
        /// Slate: answers with no question yet may be on pages a save adds
        /// (choosing a program adds its pages): apply what fits, re-read,
        /// repeat; fail if any answer is still unplaced.
        #[arg(long)]
        reveal: bool,
    },
    /// Write the live application into the profile files.
    Pull {
        /// Overwrite existing profile files.
        #[arg(long)]
        force: bool,
    },
    /// Profile checks + portal sections + programs + unapplied changes. Without
    /// -p: every portal, deadlines soonest first.
    Audit,
    /// List every course on the portal with its subject.
    Courses,
    /// Liaison: each program's prerequisites not yet ruled in
    /// prerequisites.toml, as entries to append (assigned courses filled in).
    Prereqs,
    /// Every writable question with its key, current answer and choices
    /// (--open: only unanswered ones). Liaison: each program's own questions.
    Questions {
        #[arg(long)]
        open: bool,
    },
    /// Liaison: each program's document slots with their ids and current
    /// files, for documents.toml (--open: only required slots with no file).
    Documents {
        #[arg(long)]
        open: bool,
    },
    /// Slate: query the page autocomplete service (e.g. `lookup o1 "Yale University"`).
    Lookup {
        /// The service's `type` (from the page script: `location`, `o1`, ...).
        kind: String,
        query: String,
    },
    /// Slate: print one page's HTML as the portal serves it (read-only), e.g. `get aca`.
    Get { path: String },
    /// Slate: Add Institution's school list for a region (`US:CA`) and city.
    Schools { region: String, city: Option<String> },
    /// Slate, after submission: the status checklists; saves the latest update letter to reports/.
    Status,
    /// Compare the profile with the applicant's website (`[website]` in admit.toml).
    Website,
    /// Parse a transcript and print its courses.
    Transcript {
        file: PathBuf,
        /// laccd, elmbridge or conncoll.
        #[arg(long, default_value = "laccd")]
        parser: String,
    },
    /// Manage the stored portal session cookie.
    Session {
        #[command(subcommand)]
        action: SessionCmd,
    },
    /// LACCD student portal: stored sign-in and transcript download.
    Laccd {
        #[command(subcommand)]
        action: LaccdCmd,
    },
    /// Fetch (or pick up from the inbox) a newer transcript and apply additive coursework.
    Sync {
        /// Run only on a term-end date or after `every_weeks` (what launchd runs daily).
        #[arg(long)]
        if_due: bool,
    },
    /// Every application's blockers, per program; exits 1 unless all are
    /// ready to submit (-p: one portal).
    Ready,
    /// Deadlines within --days whose program is not ready, and portals that
    /// can't be read; exits 1 when there are any (what CI opens an issue for).
    Remind {
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
    /// Liaison: each program's recommendation letters, soonest deadline first.
    Recs,
    /// Each official transcript in transcript_orders.toml with its order's
    /// status (Clearinghouse, Parchment); exits 1 if one is held or unreadable.
    Transcripts,
    /// Parchment (transcript orders): stored sign-in.
    Parchment {
        #[command(subcommand)]
        action: ParchmentCmd,
    },
    /// Liaison: compare the responses the readers use with shapes/<portal>.json.
    Drift {
        /// Rewrite the recording from the live portal instead.
        #[arg(long)]
        record: bool,
    },
    /// Install or remove the launchd job that runs `sync`.
    Schedule {
        #[command(subcommand)]
        action: ScheduleCmd,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    /// Read a Cookie header from stdin (e.g. `pbpaste | admit session set`) and store it.
    Set,
    /// Read the portal email (line 1) and password (line 2) from stdin into the Keychain and sign in.
    Login,
    /// Check that the stored session still works.
    Check,
    /// Forget the stored session.
    Clear,
}

#[derive(Subcommand)]
enum LaccdCmd {
    /// Read the user (line 1) and password (line 2) from stdin into the Keychain, then sign in once to check.
    Login,
    /// Sign in and download the unofficial transcript; prints how it parses.
    Fetch {
        /// Where to save it (default: ./LAC_SR_TSRPT-<date>.pdf).
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum ParchmentCmd {
    /// Read the email (line 1) and password (line 2) from stdin into the
    /// Keychain and sign in; the code Parchment emails is asked for on the terminal.
    Login,
}

#[derive(Subcommand)]
enum ScheduleCmd {
    Install,
    Uninstall,
}

fn main() -> Result<()> {
    // Before any thread exists; see `ctx::init_local_offset`.
    ctx::init_local_offset();
    block_on(|| tokio::runtime::Builder::new_multi_thread().enable_all().build(), run(std::env::args_os()))
}

fn block_on<T>(
    build: impl FnOnce() -> std::io::Result<tokio::runtime::Runtime>,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    build()?.block_on(future)
}

async fn run<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    execute(&Cli::parse_from(args)).await
}

async fn execute(cli: &Cli) -> Result<()> {
    // Commands that need no admit.toml.
    match &cli.cmd {
        Cmd::Transcript { file, parser } => return commands::portal::transcript(file, parser),
        Cmd::Laccd { action } => return commands::laccd::run(action).await,
        Cmd::Parchment { action: ParchmentCmd::Login } => {
            return commands::transcripts::login(
                &admit_parchment::Endpoints::default(),
                &mut commands::transcripts::tty_code,
            )
            .await;
        }
        _ => {}
    }
    let ctx = Ctx::load(cli)?;
    match &cli.cmd {
        Cmd::Audit if cli.portal.is_none() => return commands::overview::run(&ctx).await,
        Cmd::Ready => return commands::ready::ready(&ctx).await,
        Cmd::Remind { days } => return commands::ready::remind(&ctx, *days).await,
        Cmd::Recs => return commands::recs::run(&ctx).await,
        Cmd::Transcripts => return commands::transcripts::run(&ctx).await,
        Cmd::Drift { record } => return commands::drift::run(&ctx, *record).await,
        _ => {}
    }
    if ctx.portal_config()?.kind == "slate" {
        match &cli.cmd {
            Cmd::Plan { only } | Cmd::Apply { only, .. } if !only.is_empty() => {
                anyhow::bail!("--only picks Liaison sections; a Slate portal plans its answers file as a whole")
            }
            Cmd::Plan { .. } => return commands::slate::plan(&ctx).await,
            Cmd::Apply { yes, reveal, .. } => return commands::slate::apply(&ctx, *yes, *reveal).await,
            Cmd::Pull { force } => return commands::slate::pull(&ctx, *force).await,
            Cmd::Audit => return commands::slate::audit(&ctx).await,
            Cmd::Session { action } => return commands::slate::session(&ctx, action).await,
            Cmd::Status => return commands::slate::status(&ctx).await,
            Cmd::Get { path } => {
                println!("{}", ctx.slate().await?.client.get(path).await?);
                return Ok(());
            }
            Cmd::Schools { region, city } => {
                let c = &ctx.slate().await?.client;
                println!("{}", c.fetch_schools(region, city.as_deref()).await?);
                if city.is_none() {
                    println!("cities: {}", c.fetch_cities(region).await?);
                }
                return Ok(());
            }
            Cmd::Lookup { kind, query } => {
                println!("{}", ctx.slate().await?.client.lookup(kind, query).await?);
                return Ok(());
            }
            Cmd::Questions { open } => return commands::slate::questions(&ctx, *open).await,
            Cmd::Prereqs => {
                anyhow::bail!("prereqs reads Liaison programs; a Slate program's rulings are written by hand")
            }
            Cmd::Documents { .. } => {
                anyhow::bail!("documents reads Liaison programs; Slate uploads are not automated")
            }
            _ => {}
        }
    }
    dispatch_liaison(&ctx, cli).await
}

async fn dispatch_liaison(ctx: &Ctx, cli: &Cli) -> Result<()> {
    match &cli.cmd {
        Cmd::Validate => commands::portal::validate(ctx),
        Cmd::Plan { only } => commands::portal::plan(ctx, only).await,
        Cmd::Apply { reveal: true, .. } => anyhow::bail!("--reveal is for Slate portals; pick one with -p"),
        Cmd::Apply { only, yes, .. } => commands::portal::apply(ctx, only, *yes).await,
        Cmd::Pull { force } => commands::portal::pull(ctx, *force).await,
        Cmd::Audit => commands::portal::audit(ctx).await,
        Cmd::Courses => commands::portal::courses(ctx).await,
        Cmd::Prereqs => commands::portal::prereqs(ctx).await,
        Cmd::Website => commands::website::run(ctx).await,
        Cmd::Questions { open } => commands::portal::questions(ctx, *open).await,
        Cmd::Documents { open } => commands::portal::documents(ctx, *open).await,
        Cmd::Get { .. } => anyhow::bail!("`get` reads a Slate portal page; pick one with -p"),
        Cmd::Schools { .. } => anyhow::bail!("`schools` queries a Slate portal; pick one with -p"),
        Cmd::Lookup { .. } => anyhow::bail!("`lookup` queries a Slate portal; pick one with -p"),
        Cmd::Status => anyhow::bail!("`status` reads a Slate portal's status page; pick one with -p"),
        Cmd::Session { action } => commands::portal::session(ctx, action).await,
        Cmd::Sync { if_due } => commands::sync::run(ctx, *if_due).await,
        Cmd::Schedule { action } => commands::schedule::run(ctx, &cli.config, matches!(action, ScheduleCmd::Install)),
        Cmd::Transcript { .. }
        | Cmd::Laccd { .. }
        | Cmd::Ready
        | Cmd::Remind { .. }
        | Cmd::Recs
        | Cmd::Transcripts
        | Cmd::Parchment { .. }
        | Cmd::Drift { .. } => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare() -> Ctx {
        Ctx {
            profile_dir: "profile".into(),
            base: ".".into(),
            portal_name: None,
            config: ctx::Config { profile: "profile".into(), portals: vec![], sync: None, website: None },
        }
    }

    fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread().enable_all().build()
    }

    fn no_runtime() -> std::io::Result<tokio::runtime::Runtime> {
        Err(std::io::Error::other("no runtime"))
    }

    async fn finished() -> Result<i32> {
        Ok(7)
    }

    #[test]
    fn a_runtime_runs_the_future() {
        // One function-pointer type, so both the build error and the running
        // future are the same instantiation of `block_on`.
        let build: fn() -> std::io::Result<tokio::runtime::Runtime> = no_runtime;
        assert_eq!(block_on(build, finished()).expect_err("build").to_string(), "no runtime");
        let build: fn() -> std::io::Result<tokio::runtime::Runtime> = runtime;
        assert_eq!(block_on(build, finished()).expect("run"), 7);
    }

    #[tokio::test]
    #[should_panic(expected = "handled above")]
    async fn transcript_is_dispatched_before_the_portal_match() {
        tokio::task::unconstrained(async move {
            let cli = Cli::try_parse_from(["admit", "transcript", "x.pdf"]).expect("parse");
            let _ = dispatch_liaison(&bare(), &cli).await;
        })
        .await
    }

    #[tokio::test]
    #[should_panic(expected = "handled above")]
    async fn laccd_is_dispatched_before_the_portal_match() {
        tokio::task::unconstrained(async move {
            let cli = Cli::try_parse_from(["admit", "laccd", "fetch"]).expect("parse");
            let _ = dispatch_liaison(&bare(), &cli).await;
        })
        .await
    }

    #[tokio::test]
    async fn a_slate_portal_validate_uses_the_profile_check() {
        tokio::task::unconstrained(async move {
            let dir = std::env::temp_dir().join(format!("admit-main-{}", std::process::id()));
            std::fs::create_dir_all(dir.join("profile")).expect("profile");
            let config = dir.join("admit.toml");
            std::fs::write(&config, "[[portal]]\nname='s'\nkind='slate'\nbase_url='http://127.0.0.1:9'\n")
                .expect("config");
            let config_arg = config.display().to_string();
            let cli = Cli::try_parse_from(["admit", "-c", config_arg.as_str(), "-p", "s", "validate"]).expect("parse");
            execute(&cli).await.expect("validate");
            let cli = Cli::try_parse_from(["admit", "-c", config_arg.as_str(), "-p", "s", "prereqs"]).expect("parse");
            let err = execute(&cli).await.expect_err("Slate has no prerequisite list to read");
            assert!(err.to_string().contains("written by hand"), "{err}");
            std::fs::remove_dir_all(&dir).expect("cleanup");
        })
        .await
    }
}

#[cfg(test)]
#[path = "tests/main_dispatch.rs"]
mod main_dispatch;
