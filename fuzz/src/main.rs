//! `just fuzz`: a mutation fuzzer for the readers of portal pages and
//! transcripts. Seeds are the committed fixtures plus `captures/` when
//! present; every mutation stays valid UTF-8, as a decoded response is.
//!
//! Fails on a panic, an input that takes over a second, or a pulled Slate
//! answers file TOML can't read back. Pull-then-plan producing changes is
//! reported but not a failure: a mutated page often stores a value its
//! choices no longer offer, which `plan` rightly refuses.
//!
//! `admit-fuzz [seconds per target] [target]`, or
//! `admit-fuzz replay <target> <file>` to rerun a saved input with the
//! panic shown. `FUZZ_SEED` fixes the mutation sequence.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// The workspace: this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(PathBuf::new, Path::to_path_buf)
}

const SLOW: Duration = Duration::from_secs(1);

/// What a target makes of one input.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Ok,
    /// Breaks a property that a well-formed page keeps, but malformed input
    /// may legitimately break too: reported, not failed.
    Note(String),
    Fail(String),
}

type Target = fn(&str) -> Outcome;

const TARGETS: [(&str, Target); 6] = [
    ("slate", slate),
    ("html", html),
    ("laccd", laccd),
    ("website", website),
    ("transcripts", transcripts),
    ("domain", domain),
];

fn slate(s: &str) -> Outcome {
    use admit_slate::{answer, page};
    let p = page::page("frm?x", s);
    let _ = (page::nav(s), page::address_widget(s), page::widget_rows(s), page::list_rows(s));
    let _ = (page::jobs_listed(s), page::institutions_listed(s), page::degree_choices(s), page::checklists(s));
    let _ = (
        page::update_posted(s),
        page::update_pdf(s),
        page::applications(s),
        page::review(s),
        page::text(s),
        page::errors(s),
    );
    for q in &p.questions {
        let _ = (q.key(), q.writable(), q.main_input(), answer::current(q));
    }
    let _ = answer::page_post(&p, &[]);
    let repeated_id = p.questions.iter().map(|q| q.id.as_str()).collect::<BTreeSet<_>>().len() < p.questions.len();
    let pages = vec![p];
    let pulled = answer::to_toml(&pages, "# fuzz");
    let file: answer::AnswersFile = match toml::from_str(&pulled) {
        Ok(f) => f,
        // Two questions with one id (never seen from Slate) pull as a
        // duplicate key, which the read reports.
        Err(_) if repeated_id => return Outcome::Ok,
        Err(e) => return Outcome::Fail(format!("pulled answers file does not parse: {e}")),
    };
    let plan = answer::plan(&pages, &file);
    if plan.changes.is_empty() {
        Outcome::Ok
    } else {
        Outcome::Note(format!("pull then plan changes {}", plan.changes[0].key))
    }
}

fn html(s: &str) -> Outcome {
    for f in admit_html::forms(s) {
        let _ = f.has("x");
    }
    let _ = (admit_html::form_with(s, "x"), admit_html::unescape(s));
    let _ = (admit_html::attr(s, "value"), admit_html::has_attr(s, "checked"));
    if let Some(i) = s.find('<') {
        let _ = admit_html::tag_end(s, i);
    }
    Outcome::Ok
}

fn laccd(s: &str) -> Outcome {
    use admit_laccd::html;
    let _ = (html::login_error(s), html::ic_state(s), html::report_url(s));
    Outcome::Ok
}

fn website(s: &str) -> Outcome {
    let _ = (admit_website::parse(s), admit_website::published("https://example.com/", s));
    let _ = admit_website::expiry(s);
    Outcome::Ok
}

fn transcripts(s: &str) -> Outcome {
    let _ = admit_sources::laccd::parse(s, &["LAMC", "ELAC", "PIERCE", "LACC", "WLAC"]);
    let _ = admit_sources::elmbridge::parse(s);
    let _ = admit_sources::conncoll::parse(s, &["Los Angeles Mission College"]);
    let _ = admit_sources::normalize(s);
    Outcome::Ok
}

fn domain(s: &str) -> Outcome {
    for line in std::iter::once(s).chain(s.lines().take(200)) {
        let _ = admit_domain::validate::parse_date(line);
    }
    Outcome::Ok
}

/// Fragments the readers key on, and ones that break naive slicing.
const TOKENS: &[&str] = &[
    "<",
    ">",
    "\"",
    "'",
    "=",
    "/",
    "</div>",
    "<!--",
    "-->",
    "&",
    "&#",
    "&amp;",
    "&#10;",
    "&#x110000;",
    "\u{2003}",
    "\u{a0}",
    "é",
    "😀",
    "\u{0}",
    "\u{14}",
    "\n",
    "\r\n",
    " checked",
    " selected",
    "data-formula=\"",
    "@",
    "+",
    "data-readonly=\"1\"",
    "data-export=\"sys:field:x\"",
    "<div class=\"form_question\" data-id=\"z\" data-type=\"text\" data-export=\"e\">",
    "<input name=\"form_z\" value=\"v\">",
    "<select name=\"form_z\"><option value=\"a\" selected>A</option></select>",
    "</select>",
    "<p class=\"error\">",
    "<p class=\"warning\">",
    "<tr class=\"plain\">",
    "<td>",
    "<tbody>",
    "</table>",
    "<div id=\"part_checklist_",
    "<div class=\"hidden level level_",
    "was last posted ",
    "id=\"pager_download\"",
    "<tr class=\"widget_table_row\">",
    "class=\"widget_add\"",
    "<div id=\"menu\">",
    "data-href=\"",
    "?id=",
    "<h1>Review</h1>",
    "<script type=\"application/ld+json\">",
    "{",
    "}",
    "[",
    "]",
    ":",
    "null",
    "0",
    "99999999999999999999",
    "-1",
    ".00",
    "3.00",
    "Spring",
    "Fall",
    "W",
    "S*",
    "LAMC",
    "TOTAL",
    "OVERALL",
    "Credits Earned",
    "ICStateNum",
    "window.open('",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// One to eight edits of `s`: deletions, duplications, token insertions,
/// truncation, ASCII overwrites, and splices from another seed.
fn mutate(rng: &mut Rng, s: &str, seeds: &[String]) -> String {
    let mut c: Vec<char> = s.chars().collect();
    for _ in 0..1 + rng.below(8) {
        let n = c.len();
        match rng.below(7) {
            0 if n > 0 => {
                let a = rng.below(n);
                c.drain(a..(a + 1 + rng.below(64)).min(n));
            }
            1 if n > 0 => {
                let a = rng.below(n);
                let dup = c[a..(a + 1 + rng.below(256)).min(n)].to_vec();
                let at = rng.below(n + 1);
                c.splice(at..at, dup);
            }
            2 | 3 => {
                let at = rng.below(n + 1);
                c.splice(at..at, TOKENS[rng.below(TOKENS.len())].chars());
            }
            4 if n > 0 => c.truncate(rng.below(n)),
            5 if n > 0 => {
                let a = rng.below(n);
                c[a] = char::from(rng.below(0x80) as u8);
            }
            _ => {
                let other: Vec<char> = seeds[rng.below(seeds.len())].chars().collect();
                if !other.is_empty() {
                    let a = rng.below(other.len());
                    let at = rng.below(n + 1);
                    c.splice(at..at, other[a..(a + 1 + rng.below(2048)).min(other.len())].iter().copied());
                }
            }
        }
    }
    c.into_iter().collect()
}

thread_local! {
    static CATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PANIC_AT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// While [`run`] has an input, a panic records where instead of printing;
/// any other panic (a failing test) prints as usual.
fn quiet_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !CATCHING.with(std::cell::Cell::get) {
                return default(info);
            }
            let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            PANIC_AT.with(|p| *p.borrow_mut() = at);
        }));
    });
}

/// Runs `target` on `input`; a panic or a slow run is a failure.
fn run(target: Target, input: &str) -> Outcome {
    quiet_panics();
    let t0 = Instant::now();
    CATCHING.with(|c| c.set(true));
    let result = catch_unwind(AssertUnwindSafe(|| target(input)));
    CATCHING.with(|c| c.set(false));
    match result {
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            Outcome::Fail(format!("panic at {}: {msg}", PANIC_AT.with(|p| p.borrow().clone())))
        }
        Ok(_) if t0.elapsed() > SLOW => Outcome::Fail(format!("slow: {:?} for {} bytes", t0.elapsed(), input.len())),
        Ok(o) => o,
    }
}

/// One line per kind of problem: digits collapsed, so a panic's byte
/// offsets don't split it (its file:line is kept).
fn class(msg: &str) -> String {
    let (head, tail) = match msg.split_once(": ") {
        Some((h, t)) if h.starts_with("panic at") => (format!("{h}: "), t),
        _ => (String::new(), msg),
    };
    let mut out = head;
    for ch in tail.chars().take(120) {
        if !ch.is_ascii_digit() {
            out.push(ch);
        } else if !out.ends_with('#') {
            out.push('#');
        }
    }
    out
}

fn seeds() -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                walk(&path, out);
            } else if matches!(path.extension().and_then(|x| x.to_str()), Some("html" | "txt" | "json" | "xml")) {
                out.extend(std::fs::read_to_string(&path));
            }
        }
    }
    let root = root();
    let mut out = vec![String::new()];
    walk(&root.join("captures"), &mut out);
    for c in std::fs::read_dir(root.join("crates")).into_iter().flatten().flatten() {
        walk(&c.path().join("tests/fixtures"), &mut out);
    }
    out
}

fn target(name: &str) -> Option<Target> {
    TARGETS.iter().find(|(n, _)| *n == name).map(|&(_, t)| t)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [cmd, name, file] = args.as_slice()
        && cmd == "replay"
    {
        let (Some(t), Ok(input)) = (target(name), std::fs::read_to_string(file)) else {
            eprintln!("replay <{}> <file>", TARGETS.map(|t| t.0).join("|"));
            return ExitCode::FAILURE;
        };
        let t0 = Instant::now();
        let outcome = t(&input);
        println!("{outcome:?} in {:?}", t0.elapsed());
        return ExitCode::from(u8::from(matches!(outcome, Outcome::Fail(_))));
    }
    let secs = args.first().and_then(|a| a.parse().ok()).unwrap_or(30);
    let only = args.get(1).map(String::as_str);
    if only.is_some_and(|o| target(o).is_none()) {
        eprintln!("unknown target; known: {}", TARGETS.map(|t| t.0).join(", "));
        return ExitCode::FAILURE;
    }
    let seed = std::env::var("FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1
    });
    let out_dir = root().join("target/fuzz");
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("{}: {e}", out_dir.display());
        return ExitCode::FAILURE;
    }
    let seeds = seeds();
    println!("{} seeds, FUZZ_SEED={seed}, {secs}s per target", seeds.len());
    let mut rng = Rng(seed);
    let mut failed = false;
    for &(name, t) in TARGETS.iter().filter(|(n, _)| only.is_none_or(|o| o == *n)) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut seen: BTreeMap<(bool, String), usize> = BTreeMap::new();
        let mut runs = 0usize;
        // The unmutated seeds first, then mutations until the deadline.
        while runs < seeds.len() || Instant::now() < deadline {
            let input = match seeds.get(runs) {
                Some(s) => s.clone(),
                None => {
                    let k = rng.below(seeds.len());
                    mutate(&mut rng, &seeds[k], &seeds)
                }
            };
            runs += 1;
            let (fatal, msg) = match run(t, &input) {
                Outcome::Ok => continue,
                Outcome::Note(m) => (false, m),
                Outcome::Fail(m) => (true, m),
            };
            let n = seen.entry((fatal, class(&msg))).or_default();
            *n += 1;
            if *n == 1 && fatal {
                let file = out_dir.join(format!("{name}-{}.txt", seen.len()));
                if std::fs::write(&file, &input).is_ok() {
                    println!("  {msg}\n    input: {}", file.display());
                }
            }
        }
        failed |= seen.keys().any(|(fatal, _)| *fatal);
        println!("{name}: {runs} inputs");
        for ((fatal, kind), n) in &seen {
            println!("  {} {n:>7}  {kind}", if *fatal { "FAIL" } else { "note" });
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panics(_: &str) -> Outcome {
        let s = "é";
        let _ = &s[1..];
        Outcome::Ok
    }

    fn sleeps(_: &str) -> Outcome {
        std::thread::sleep(SLOW + Duration::from_millis(50));
        Outcome::Ok
    }

    #[test]
    fn a_panic_is_a_failure_that_names_its_line() {
        let Outcome::Fail(msg) = run(panics, "") else { panic!("not caught") };
        assert!(msg.contains("fuzz/src/main.rs:"), "{msg}");
        assert!(class(&msg).contains("main.rs:"), "file:line survives the digit collapse: {}", class(&msg));
    }

    #[test]
    fn a_slow_input_is_a_failure() {
        assert!(matches!(run(sleeps, ""), Outcome::Fail(m) if m.starts_with("slow")));
    }

    #[test]
    fn mutations_are_deterministic_for_a_seed() {
        let seeds = vec!["<div class=\"a\">é😀</div>".to_string()];
        let a: Vec<String> = (0..50).scan(Rng(7), |r, _| Some(mutate(r, &seeds[0], &seeds))).collect();
        let b: Vec<String> = (0..50).scan(Rng(7), |r, _| Some(mutate(r, &seeds[0], &seeds))).collect();
        assert_eq!(a, b);
        assert!(a.iter().any(|m| m != &seeds[0]));
    }

    #[test]
    fn the_committed_fixtures_pass_every_target() {
        let seeds = seeds();
        assert!(seeds.len() > 20, "fixtures found: {}", seeds.len());
        for s in &seeds {
            for &(name, t) in &TARGETS {
                assert!(!matches!(run(t, s), Outcome::Fail(_)), "{name}");
            }
        }
    }

    #[test]
    fn a_pull_with_a_duplicated_question_id_is_not_a_failure() {
        // Slate never repeats an id; a mutation that copies a question does,
        // and its pull then holds a duplicate key the read reports.
        let q =
            "<div class=\"form_question\" data-id=\"a\" data-type=\"text\"><input name=\"form_a\" value=\"v\"></div>";
        assert_eq!(slate(&q.repeat(2)), Outcome::Ok);
    }
}
