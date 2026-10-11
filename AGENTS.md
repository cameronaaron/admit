# Agent notes — admit

Declarative school-application automation, modelled on `~/homestead`'s
provisioners: `profile/*.toml` is the source of truth, `admit plan` diffs it
against a portal, `admit apply --yes` converges the portal and re-plans to
prove it (no changes left *and* no violation in the selected sections,
else it fails: `ctx::settle`), `admit pull` writes the portal back into the profile. Nothing
outside the profile is ever deleted.

What is proven, per operation and deployment (live / captured / bundle /
no), is `SUPPORT.md`; update its row when a write is first seen to work.

Gate: `just check` (fmt, clippy `-D warnings`, tests). A fix lands with its
regression test.

## Layout

- `crates/domain` — profile types (`spec.rs`), `Finding`, and portal-independent
  checks in `validate/`: one file per section (`experiences`, `degrees`,
  `licenses`, `high_school`) plus `cross.rs` for checks spanning sections
  (a degree dated before the high-school diploma). Licenses expiring within
  90 days are Pending, expired ones Violations.
- `crates/sources` — transcript parsers (`laccd`, `elmbridge`, `conncoll`) and
  HAR cookie extraction.
- `crates/liaison` — Liaison CAS adapter. `sections/` has one module per
  application section (`live` / `plan` / `apply`); `plan.rs` the `Plan` and
  its rendering; `reconcile.rs` the shared keyed diff; `audit/` one module
  per portal check (navigation, deadlines, evaluations, fees, transcripts,
  prerequisites, attendance), each a pure function of the endpoint's JSON
  with a test of the captured shape.
- `crates/liaison/src/drift.rs` + `shapes/nursingcas.json` — `admit drift`:
  every key path and JSON type (never a value) of the 35 responses the
  readers use (per-program ones merged over the selected programs), recorded with `--record` and compared live. `apply` and `sync` compare the responses the written sections are
  built from (`drift::reads_for`) before their first write and refuse on
  any break, or when the portal has no recording. Readers turn a
  missing field into `""`/`0` silently; this is what notices. A key that is
  not an identifier is data and becomes `*` (`collegeMap` by id, `orgMap`
  by school name). The portal leaves an empty list out instead of sending
  `[]` (achievements, 2026-10-06 capture vs 2026-10-07 live), so a missing
  list is a Note unless a new key appeared beside it. Validated by checking
  the 2026-10-06 captures against the live recording: that Note only.
- `commands/ready.rs` — `admit ready` (per program: blockers = Violation +
  Pending from the audit, minus a deadline that is merely near) and
  `admit remind --days N` (unready programs due within N days; exits 1).
  A finding belongs to the program its message starts with, else it blocks
  every program on that portal. `admit recs`: recommendation counts per
  program joined to deadlines on `programSelectedId`; names of recommenders
  are not read (no capture of a requested letter yet).
- `.github/workflows/watch.yml` — daily `drift` + `remind`, each keeping one
  issue (label `admit-watch`) updated and closing it when clear. Needs
  `UCLA_*`/`APU_*` secrets besides `NURSINGCAS_*`.
- `crates/laccd` — LACCD student portal: ADFS sign-in and the unofficial
  transcript download (`admit laccd fetch`, used by `admit sync`).
- `crates/html` — tag/attribute/form readers shared by the HTML portals.
- `crates/clearinghouse`, `crates/parchment` — transcript ordering services,
  read only: order status into `admit_domain::orders::Order`.
  `admit transcripts` (`commands/transcripts.rs`) checks every send in
  `profile/transcript_orders.toml` against them (`orders::check`); a
  service it can't read is UNCHECKED and its sends are left out, never
  reported "not ordered". `admit parchment login` stores the sign-in.
- `crates/slate` — Slate application portals (UCLA): `page.rs` reads the
  menu, questions and Review page; `answer.rs` plans `profile/slate/<name>.toml`
  against them; `lib.rs` saves and audits.
- `crates/website` — the profile against the applicant's site's schema.org
  JSON-LD (`admit website`, also part of `admit audit`).
- `crates/cli` — the `admit` binary: `main.rs` is the clap tree and dispatch
  only; `ctx.rs` config/profile/session; `commands/` one file per command
  group; `sync.rs` the pure scheduling and transcript-diff logic.
  `admit audit` without `-p` (`commands/overview.rs`) audits every portal:
  deadlines from all portals soonest first (Slate has none on its pages,
  so `deadline = "YYYY-MM-DD"` under its `[[portal]]`), then each portal's
  violations and pending items; an unreadable portal is UNCHECKED.
  `profile/prerequisites.toml` holds each program's rulings (course per
  requirement, `within_years`, `deferred`); `prerequisites.rs` checks them
  against the parsed transcripts in that program's portal audit. Codes
  match ignoring spaces and leading zeros ("PSYCH 41" = "PSYCH 041"); an
  unreadable transcript (CI has only LACCD's) is Pending, not a failure.
- `fuzz` — `just fuzz [secs per target] [target]`: mutation fuzzer over the
  page and transcript readers, seeded from fixtures and `captures/`. Fails
  on a panic, an input over 1 s, or an unreadable pulled answers file;
  saves each failing input to `target/fuzz/`, rerun with
  `cargo run -p admit-fuzz -- replay <target> <file>`. Outside `crates/`,
  so the coverage gate skips it; `just check` still builds and tests it.
- `captures/` (gitignored) — what the deleted 2026-10-06 HARs taught, minus
  credentials: `nursingcas/<METHOD>_<path>.json` holds one request/response
  body per read endpoint and every captured write; `laccd/flow.json` is the
  sign-in + transcript flow with secrets redacted. Read these before
  re-capturing anything.

## Interrupted writes

Every portal write (NursingCAS non-GET, Slate saves) is journalled in
`reports/journal-<portal>.log` (`admit_domain::journal`): a `started` line
before it is sent, `done`/`failed` when answered; a write whose `started`
line can't be written is not sent. An apply that errors names that file.
The only destructive step, a document replace, first saves the held file
to `profile/documents.replaced/<attachment id>-<name>` and records itself
in `documents.lock.json` (`replacing`), then DELETEs. If the new upload
fails, the saved file is uploaded again and the error says whether that
worked; if the run dies, a plan that finds the slot empty names the saved
file, and uploads.

## Liaison API facts (captured 2026-10 from HARs + the applicant JS bundle)

- Every Liaison CAS uses `https://<cas><cycle>.cas.myliaison.com/applicant/api`.
- Auth is the browser `SESSION` cookie; there are no tokens. Stored in the
  macOS Keychain (`admit session set`, service `admit`, account = portal).
- Sign-in (captured 2026-10-06, `captures/nursingcas/LOGIN_flow.json`):
  `GET /applicant/initialize` sets `cas`, `casName`, `formId`; then
  `POST /applicant/authenticate` with HTTP Basic `email:password`, body `{}`,
  sets `SESSION` ("Successful Authentication"). No MFA. `Client::login`;
  the email/password live in the Keychain as `<portal>-user` /
  `<portal>-password` (`admit session login`), and `Ctx::portal` signs in
  again whenever the stored cookie answers 401.
  An expired session answers **401** with JSON ("Full authentication is
  required") — observed 2026-10-06. (An earlier note here claimed a 200 HTML
  login page; that was a guess, never observed.) Any `<`-prefixed body is
  also treated as expiry.
- Licenses have no PUT: the form POSTs multipart with a JSON part named
  `model` (filename `blob`); `id: 0` creates, an existing id edits.
- Experiences are question blocks. Edit = PUT `/{id}` with *every* block;
  sending only the changed ones would drop domains/competencies/"most
  important". Create = POST without id.
- A college's degrees are saved together in one PUT of the whole college.
  `degreeStatusId` 431 = Awarded, 430 = Expected (observed; no lookup seen).
  The server rejects an Expected date that is not in the future.
- New terms: POST `/saveOrUpdateTermAndCourses/{collegeId}` with
  `{termId, yearId, academicStatusId, termInProgress, id: null, courses: [], year}`;
  the response carries the new term id. Courses then POST to
  `/academicHistory/courses/collegeId/{c}/termId/{t}`.
- Sections are marked done with
  `PUT /updatecheckliststatus/subSection/{id}/status/true/optOut/false` —
  the "Done" button; not a submission.
- Add an institution: `POST /academicHistory/colleges` (body in
  `coursework::create_college`; the wire test pins it to the body the form
  sent when Pierce was added by hand). Edit: `PUT /academicHistory/colleges/{id}`;
  delete: `DELETE /academicHistory/colleges/{id}`.
- High school: attendance rows `GET /academicHistory/highSchools` (`data[]`),
  edit `PUT /academicHistory/highSchools/{id}` = the row + form fields (see
  `high_school::attendance_body`; worked live 2026-10-06), delete `DELETE …/{id}?subSectionName=…`.
  Diploma: `GET /academicHistory/highSchoolInfo/{sectionId}` then
  `POST /academicHistory/highSchoolInfo?subSectionName=highSchoolAttended`
  with the whole document, `graduationStatus.data =
  {diplomaOrEquivalency, diplomaOrEquivalencyDate: "YYYY-MM", highSchoolName}`
  (captured, 200).
- Liaison month ids are not 1..12: Jan–Apr 1–4, May 8, Jun 9, Jul 10,
  Aug 11, Sep 12, Oct 13, Nov 14, Dec 15. Year ids are arbitrary (2013 = 19,
  2017 = 5745, 2026 = 12216): always resolve through `/lookup/year`.
- Deletes (bundle; reconcile never calls them): `DELETE
  /supportingInfo/achievements/{id}`, `/supportingInfo/licenses/{id}`,
  `/supportingInfo/experiences/{id}` (captured 200).
- Course edit (from the applicant bundle, 2026-10-06; verified live the same
  day: it fixed four title typos, and the live re-plan of 2026-10-07 is 0
  changes): `PUT /academicHistory/courses/collegeId/{c}/termId/{t}/courseId/{id}`
  with the create form's fields (`code`, `title`, `grade`, `gradeEnabled`,
  `convertedGrade`, `termId`/`subjectId` as strings, `credits`,
  `courseClassification` flags, `source`). Planned only for a mistyped code
  (same term, title abbreviates the transcript's); unattended `sync` never
  applies it.
- Subjects: `[transcript.subjects]` in coursework.toml applies to courses
  already entered too (exact transcript code): a differing portal subject is
  corrected with the course PUT. `admit courses` lists every course with its
  subject; `/lookup/subjects` marks which subjects are science (`isScience`).
  Don't move a course into a science subject without a reason a program
  would accept. A matched course whose code differs in the same term (ELAC
  "POL SCI" for "POL SCI 019") gets the transcript's code.
- "Still attending" is set per institution from the transcript's college
  column: true while an ungraded course is listed there, else false ending
  at the last listed term. Written with the college PUT (`education::college_body`,
  degrees re-sent unchanged).
- Deadlines: only `POST /search/filteredQueries` with
  `querySelectedPrograms: true` (body = `/programsSelected` plus fixed
  flags) returns every selected program's deadline;
  `/academicHistory/transcriptRequirements` lists only some. Each program has
  `deadline` and `deadlineDisplay`, which disagree (Johns Hopkins MSN:
  display "Jan 15, 2027", field 2027-03-16); the audit uses the earlier.
- Recommendations: `GET /programMaterials/evaluations/programSelected/{programSelectedId}`
  (`evaluationTypes[].minRequired`, `evaluationsRequested`,
  `evaluationsCompleted`). `evalReqLastDate` is the cycle end for every
  program (2028-01-28), not a per-program date.
- Official transcripts: `GET /transcripts` → `college.collegeMap[*]`
  (`orderedTRF`, `officialTranscript.transcriptReceived`). A still-attending
  college reports `collegeEndMonth: "Still Attending"` in `/academicHistory/courseWork`.
- Application-level form sections (biographic, contact, citizenship, other
  info, family, release): `GET /applicationFormSubSectionBlocks/subSection/{id}`
  returns question blocks (`questions[{id, shortFieldName, value, …}]`);
  saving is `PUT` (or `POST` the first time) of the same document. Not
  modelled in the profile yet.
- Personal details are compared, read-only, with `personal.toml` in every
  portal's audit (`validate/personal.rs`; NursingCAS `liaison/personal.rs`
  reads Biographic + Contact Information and `/authenticatedUser` email;
  Slate `slate/personal.rs` reads the answers file, which audit proves is
  on the portal). A GET of these sections does not carry a lookup-backed
  dropdown's chosen option: its one `answerOptions` entry names the table
  (`LK_Languages`); only the PUT response echoes the option. Country,
  state and county resolve through `/lookup/*` (text and `code`). Building
  the reader from the PUT shape first reported region/country "blank".
- Program prerequisites (captured 2026-10-07, California Baptist and Yale;
  `sections/prereqs.rs`): `GET /programMaterials/{programSelectedId}/prerequisites`
  (requirements with `minGradeAlphabetic`, `minCredits`, assigned `course[]`),
  only for programs whose navigation entry has `selectedProgramHasPreReqSection`;
  the course list is `GET /programMaterials/prerequisites/transcripts`. Assign =
  `POST` of the GET document minus `instructions` and each requirement's
  `name`/`description`, every assigned course as eight form fields
  (`gradeValue` is the *credits*, "3.00"), one course per POST on the previous
  response. A POST response has `name: null`, so re-plan from a GET. The body
  must be written as the browser's `JSON.stringify` does: the GET's
  `minGrade: 3.0` is posted as `3` (the only difference among 12 replayed
  posts before `js_numbers`). `admit -p nursingcas prereqs` prints unruled
  programs for `prerequisites.toml`.
- Program Questions (captured 2026-10-07, Yale and UC Davis; `sections/questions.rs`,
  `profile/questions.toml` keyed by question id, choices by option text):
  `GET /programMaterials/{programSelectedId}/questions` (question groups;
  answers are option ids in `value[]`, and unlike Biographic Information the
  GET carries the chosen option). Save = `POST …/questionsPartialSave` with
  the document reshaped (`body()`'s doc lists every rule: no question text, a
  blank value as `[""]`, only the chosen option; a textarea is `value:
  ["<text>"]`, `answerOptions: []`). A form with nothing required left blank
  is saved the other way the browser does (UC Davis, 2026-10-07): the same
  body to `POST …/questions`, then a body-less `PUT
  /updateProgramMaterialsStatus/programSelected/{id}/section/{supplementalSectionIds.Questions}/optOut/false/completionStatus/true`
  (`Client::put_empty`: the captured PUT has no body and no Content-Type).
  Every captured save replays byte for byte (6 question saves, one clearing
  two typed answers with `""`, and 16 prerequisite posts, as of 2026-10-07). The browser first POSTs the
  choice answers (not the typed ones) to
  `/extendedProfile/applicationProgramSelectionImpact`, which says whether
  they would remove or conflict with other program selections; `apply`
  refuses to save on any reported impact. Written: radio, dropdown, textarea
  (within `maxLength`); other kinds were never captured. `admit -p nursingcas questions --open` lists what is
  unanswered with its choices. `GET …/orgQuestions` (school-level) is not read.
- Program Documents (`sections/documents.rs`, `profile/documents.toml` keyed
  by document slot id, values = file paths from the profile dir; `admit -p
  nursingcas documents [--open]` lists the slots). Upload, from the bundle's
  `US` and one capture (Yale, 2026-10-07): multipart `POST
  /programMaterials/{ps}/documents/{documentId}/{sectionId}/1/{"In Progress"
  if the program's navigation `status` is INCOMPLETE, else
  "Complete"}/{statusCompleted 0|1}`, parts `file` then `model` (`blob`,
  JSON = the section GET with `optOut: false`). Delete (`UC`): `DELETE
  …/documents/{documentId}/{attachmentId}/{sectionId}/1/{program status}`.
  The `1` is a literal in the bundle. A slot holds one file; a different
  file name in the profile replaces it (delete, re-GET, upload), as the
  browser re-reads the section after each write. The section GET carries
  only `{id, fileName, uploadDate}`; `GET
  /programMaterials/documents/view/{attachmentId}` serves the file itself
  (captured 2026-10-07), and plan compares a same-name file by its SHA-256.
  Validated live 2026-10-08: all 8 held CVs hashed equal to the local file
  (0 changes), and one appended byte made all 8 plan as "the portal's copy
  differs". This replaced a local hash record of what admit had uploaded,
  which could not see a file uploaded by hand and, with the lock missing,
  planned all 8 CVs for delete-and-reupload. A same-name file absent here
  (CI) is Pending, never replaced. The picker takes 1 KB-15 MB
  (`0xf00000`), pdf/doc/docx/rtf/txt/png/jpeg/heic/heif, no password-protected
  PDF. Marking Documents done reuses the Questions PUT on the Documents
  subsection id (= the GET's `sectionId`); it worked live 2026-10-07 (Yale,
  JHU x2, Rush), but done is read from the navigation's
  `subSections[].completed`: the documents GET's `statusCompleted` stayed
  false, which first made the re-plan report "did not converge". wiremock's `path()` never matches a path with `%20`: tests
  compare `url.path()`.
- `/applicationFormSubSectionBlocks/subSection/{id}` returns *empty* blocks
  for program-level Questions subsections: program questions use another
  endpoint, not yet captured.

## Slate facts (UCLA Graduate Division, captured 2026-10-06)

`captures/ucla/` holds what the three 2026-10-06 HARs taught (the HARs
themselves are deleted; they held the password): `flow.json` (first HAR),
`har2/flow.json` + pages (the whole later session: switching applications,
logout, sign-in), `login_and_open_flow.json`, and pages fetched live. Cookie
values, email and password are removed; read these before re-capturing.

- Pages are `/apply/frm?<guid>`, listed by the menu (`<div id="menu">`,
  `apply_navigate` links). `aca` (institutions), `job` (employers), `tes`
  (tests), `certify` (signature) and `review` are not question pages.
- A question is `<div class="form_question" data-id data-type data-export>`;
  its controls are `form_<data-id>` (dates: `_m`, `_d`, `_y`, two-digit
  month/day values). `data-export` (`sys:field:gd_birth_country`) is the
  stable key, except where several questions share one: three Statement of
  Purpose textareas (one per program) do, so they are keyed by id.
  `data-readonly="1"` + `data-formula` marks a copy the browser computes
  (lived name = legal name): never written.
- Both UCLA applications are kept in sync (asked 2026-10-07): portal
  `ucla-2026` has `same_as = "ucla"`, sharing ucla's Keychain sign-in,
  session and `profile/slate/ucla.toml`; `pull` refuses on it. A blank
  application has no program pages until `gd_major_program` is saved, so
  its first run is `admit -p ucla-2026 apply --reveal --yes`: save what
  fits, re-read, repeat (two rounds live) and fail if an answer is still
  unplaced. Without `--reveal` an unplaced answer still blocks, as a typo.
- Slate renders `checked` on two radios of a group (default + stored); the
  browser shows the last. Read the last.
- Save = multipart `POST frm?<guid>` with `dirty=1` and the fields; the
  answer is a `parent.FW.Navigate(…)` script. `&r=<page>` only picks where
  the browser goes next. **Slate clears the page's fields a save leaves
  out** (live, 2026-10-06), so `answer::page_post` posts the whole page as
  the browser does: every question's current value, the changes swapped
  in, `data-formula` copies computed. Checked against the browser's Basic
  Information post: identical but for three hidden lived-name fields sent
  with their stored values. Pages with uploads (`plugin:material`) are
  refused. `apply` still re-reads each saved page and stops if an untouched
  answer moved.
- Addresses (Basic Information) are a widget saved apart from the page:
  the table rows (`tr.widget_table_row`: type, street, city, region code,
  country *name*, postal) and an "Insert Address" link
  (`a.widget_add data-href=/apply/frm?cmd=widget&form=…&field=…&scope=app_hosted_page`).
  Insert = multipart POST to that URL: `form_<type q>` = `mailing` |
  `permanent`, `form_<address q>_country` (code, `US`), `_street`, `_city`,
  `_region` (code), `_postal`, `_quality=0`, `_dirty=1`, `dirty=1`,
  `cmd=save`; answered with a script that re-fetches the question via
  `frm?<page>&cmd=refresh&field=<widget id>`. The country/region selects are
  filled by script, so their codes can't be read from the form. Editing a
  row (`…&id=<row id>`) was never captured: admit reports differences.
  Optional Valid From/To dates (`sys:address:effective`/`expires`) unused.
  The browser's address autocomplete (`fw.cdn.technolutions.net/address?cmd=suggest|verify`)
  is not needed for a save.
- Expired session: `302` to `/account/login?r=…`. One idle for about ten
  minutes was already expired.
- Sign-in (`Client::login`, `captures/ucla/login_and_open_flow.json`):
  `GET /account/login?r=…` → `307` to `…&cookie=1` setting `_cookie_test`,
  `_hash`, `_hashV`, `_node` (skip that round-trip and it answers `503`);
  then `POST /account/login?r=…` urlencoded `email`, `password` → `302` to
  `r` setting `_uid`. No MFA. Keychain `ucla-user` / `ucla-password`
  (`admit -p ucla session login`); `Ctx::slate` signs in again on expiry.
- **Every POST needs `Origin`.** Without it sign-in still answers `302` to
  `r` but sets no `_uid`, and any other POST is redirected to sign-in - it
  reads exactly like an expired session while the session is fine (checked
  live 2026-10-06; cost one wrong diagnosis).
- The account can hold several applications (`/apply/` "Your
  Applications", rows `data-href="?id=<id>&cmd=detail"`); every `frm?` page
  reads and writes the one named by the `.application` cookie. UCLA has
  2027-2028 and 2026-2027 applications; the second HAR shows
  pages being saved into the 2026-2027 one after it was opened. Opening =
  `POST /apply/?id=<id>&cmd=detail` body `cmd=save` → `Set-Cookie:
  .application=<id>`. `[[portal]] application = "<id>"` pins it, and
  `Client::open` fails unless the cookie comes back with that id.
- Review (`/apply/review`): an error table (`<p class="error">`, blocks
  submission) and a warning table (`<p class="warning">`), rows of
  section link + message. `audit` reports them as violations / pending.
- Not automated (save never captured): Add Institution (`aca?id=` - a
  region → city → school lookup by AJAX, then a transcript upload), Add
  Employer (automated since; see above), tests (`tes?id=`),
  uploads (`material_<guid>` file parts), the signature (`certify`:
  `signature0`, `signature`; POSTing it empty only navigates). The second
  HAR (2026-10-06 20:44) has no such save either.
- After submission (Azusa Pacific, `engage.apu.edu`, captured 2026-10-06;
  `captures/apu/`): `/apply/review` redirects to `/apply/status` and
  `/apply/` to a custom `/portal/app`, so there is nothing to open or edit
  (`submitted = true`). The status page holds checklists
  (`<div id="part_checklist_<Name>"><h3>…</h3><table>`, rows `tr.plain`:
  status icon, status, detail, date) and "An update to your application
  was last posted <date>." with a link to `/apply/update`, whose PDF is
  `update?cmd=stream&id=<id>` (`#pager_download data-href`). APU's update
  was its prerequisite evaluation. Sign-in is the same form as UCLA's.
  Captured pages carry `data-session-token` in a meta tag: redacted in
  `captures/`.
- Academic History (`aca`, `institutions.rs`): Add Institution's school
  list comes from `aca?cmd=fetch_cities&region=US:CA` (regions with >25
  schools) and `aca?cmd=fetch_schools&region=…&city=…` (`[key, name, city,
  region]`), per `aca.js`; `admit -p ucla schools <region> [city]` queries
  it. The save is a multipart POST to `aca?id=` with `key`, `name`,
  `city_keyed`, `region_keyed`, `country_keyed`, `from_m/_y`, `to_m/_y`,
  `type` (U/G), `degree` (the level's option value), `conferred_m/_y`,
  `cmd=save` and the transcript as `file1`; worked live 2026-10-06 (the edit
  form then says "We have successfully received your transcript"). A school
  not on the list goes in as "Other": empty `key`, plus `country`/`city`/
  `region` (Elmbridge University, Lehi UT). One degree per entry.
- Professional Experience (`job`, `jobs.rs`): Add Employer posts urlencoded
  (the form has no enctype; its Save is `FW.Lazy.Commit(this)`, no `cmd`) to
  `job?id=`: `employer`, `from_m/_y`, `to_m/_y` (empty `to_m` = Present),
  `title` ≤64, `hours` ≤2 digits, `description` ≤1000. Worked live
  2026-10-06; the list shows only employer and "MM/YYYY–MM/YYYY", so roles at
  one employer with the same dates are matched by count.
- Each added school then appears on Additional Academic Info as its own
  popup form (`frm?cmd=widget&form=…&id=<school>`), saved like a page plus
  `cmd=save`: major (`sys:school:major`, max 32 chars), GPA, GPA scale, and
  From/To as `dateym` (month + year selects, no day). Review says "One or
  more schools are missing GPA, GPA Scale or Major" until every school has
  all three (a school with only in-progress courses took "N/A").
- Questions sharing an export key share one stored value: the Award
  Application's Cota-Robles and Graduate Opportunity Program questions both
  write `gd_div_fellowship`, so answering both leaves only the last. `plan`
  refuses two answers that resolve to one export key (a Violation, neither
  written). Answer the one that applies (GOP for master's).
- The LACCD transcript prints each term's dates ("2026 Spring (Feb 9, 2026 -
  Jun 8, 2026)"): use them for completion dates instead of estimates.
- Autocomplete fields (`new Suggest({ input, service: '…/manage/service/lookup?type=<kind>&q=' })`
  in the page script; Survey's institutions use `type=o1`) answer JSON
  `{"item":[{"name": "<b>Yale</b> University", "key": "003987", …}]}`.
  Picking one sets the visible name (tags stripped) and a `data-invisible`
  code field; a name without its code is "Institution not recognized" on
  Review. `admit -p ucla lookup o1 <text>` queries it; the answers file sets
  both name and code. The list abbreviates ("Uc Davis", "Csu East Bay") and
  lacks some schools (no Johns Hopkins nursing, no MGH Institute).
- Other endpoints seen, not used: `/portal/landing` (account home),
  `POST /apply/review` `cmd=return` (back to the application list),
  `/account/logout?r=…`, an embedded `/register/` form, and
  `grad.ucla.edu/apps/smartrecscore/api/main/` (unrelated; anonymous 400).

## Transcript ordering facts (captured 2026-10-10)

`captures/clearinghouse/{order,track,verify-signup}/` and
`captures/parchment/` hold one file per request, SSN, card, passwords, codes
and tokens removed (`verify-signup` is the Verify portal account sign-up,
not used by admit). The six HARs they came from held the SSN, the card's
CVV and first six digits, and passwords; they were deleted 2026-10-10 after
a scan found none of those in `captures/`. Read these before re-capturing.

- Clearinghouse ordering is anonymous: `POST student/check` with name, DOB
  and the full SSN, `POST order`, `recipient/preprocess` (fee quote),
  `order/{id}/recipients`, card tokenised by Clover's iframe
  (`token.clover.com/v1/tokens`, PAN encrypted, CVV in clear), `POST
  order/{id}/payment` with the `clv_` token, `POST
  order/{id}/consent-signature` with the drawn strokes (`lines`). The
  bearer arrives in the `Authorization-Token` response header of `POST
  order` / `POST trackorder`.
- Clearinghouse tracking: see `crates/clearinghouse` docs. Every answer is
  HTTP 200; failures are `"status": "fail"`. The summary carries the full
  SSN: never print or store a tracking response.
- Clearinghouse's front end answers **403 without a User-Agent** and **415
  to `content-type`** (lowercase, which hyper sends): reqwest needs
  `.http1_title_case_headers()`. wiremock can't see header case; the wire
  test reads raw bytes from a TcpListener.
- Parchment sign-in: see `crates/parchment` docs. The one-time code goes to
  the account email (the LACCD student address), not an app.
- Parchment orders are in `/lp/orders`'s Next.js flight data, not an API.
  `api.parchment.com/graphql` (bearer = `/lp/api/auth/session`
  `accessToken`) was seen only for per-delivery print manifests, tax and
  school search. Placing an order is a Camunda process: `POST
  page-workflow-api/engine-rest/process-definition/key/PageOrderWorkflow/start`,
  then each `task` is read (`formKey`: `selectReceiver`, product details,
  order summary) and `complete`d with its variables; payment is a form POST
  to `payment.parchment.com` (`chkSum`, `invoiceNumber`, `callbackUrl`) and
  Stripe Checkout. The captured order (CSUN to LACC, $6.35) stopped at
  Stripe; nothing after payment was seen.

## Essays

`profile/about.md` is the fact base for answers the TOML files don't hold
(intent, grade explanations, never-write claims, recommenders, open
questions), built from the profile, the applicant's website and their essay
notes, which live outside this repository. Essay drafts are the applicant's
works in progress: mine
them for facts, never copy them into an answers file or document slot as
final text. An essay slot keeps its placeholder until the applicant
supplies the text; a short placeholder answer is Pending
(`questions::plan`), so `admit ready` keeps the program blocked.

## Not yet verified against the live API — don't automate blind

- Posting grades into an in-progress term (term → completed): the request
  shape is in the bundle (`saveOrUpdateTermAndCourses` with the term id and
  graded courses) but has never been seen to succeed. The coursework plan
  reports posted grades as Pending instead.
- Requesting a recommendation (`sections/references.rs`, `profile/references.toml`):
  built from the bundle's evaluation form (`vh` body, `POST
  /programMaterials/evaluations/programSelected/{ps}`, preceded by the
  form's `GET /emailVerification/pooledEvaluations/email/{email}/evaluationType/{type}`),
  never captured. Each request emails a real person: a wrong body costs a
  confusing email, so capture the first one and compare before relying on
  it. The due date's format is inferred (`parseISO` validation means
  `YYYY-MM-DD`). Planned only with `consent = true` and never for an
  address the program already lists.

## GitHub Actions

- `check.yml`: the `just check` gate on every push.
- `sync.yml`: `admit sync` weekly and on the term-end dates, with
  credentials from repo secrets `LACCD_USER`/`LACCD_PASSWORD`/
  `NURSINGCAS_USER`/`NURSINGCAS_PASSWORD` (`session::credentials` reads
  `<NAME>_USER`/`<NAME>_PASSWORD` before the Keychain). Only the LACCD PDF
  exists in CI (others are gitignored); sync skips absent transcript files.
  The last transcript is kept in the Actions cache so the report shows what
  changed. Actions are pinned to commit SHAs (`zizmor --offline .` clean).

## Transcripts

- Parsers: `laccd` (district transcript, all LACCD colleges, reconciled per
  term), `elmbridge` (reconciled to "Credits Earned"), `conncoll` (Banner;
  transfer rows head-labelled by the sending school for one term; reconciled
  to "OVERALL" attempted, which excludes the Spring 2020 `S*` grade — that
  term's own summary shows 6 of 18 credits attempted).
- The Bridges Academy (high school) transcript is a reference for
  `high_school.toml`, not coursework.
- Course matching is one-to-one (a retaken course is two rows; MAT 210 W then
  S*), same term first. A row whose title abbreviates a same-term portal
  course's title, with a different code, is a mistyped code: reported, never
  added (the portal had PSY 323 for the transcript's PSY 321).
- LACCD transcript retrieval is automated (`crates/laccd`, flow in its
  module doc). Facts from the 2026-10-06 capture: ADFS forms sign-in with no
  MFA, user sent as `escsso\<student id>`; two auto-posting relay forms
  (SAML, then PeopleSoft `PrincipalRequest`); every PeopleSoft postback must
  echo the latest `ICStateNum` and `ICSID`; the report link arrives as
  `window.open('…/LAC_SR_TSRPT.pdf')`. Sign-in lives in the Keychain
  (`laccd-user`, `laccd-password`). A fetched PDF differs from the previous
  one only in "Print Date", so sync installs a transcript only when
  `transcript_changes` (course-level diff) is non-empty.
- The course-history page (`SSR_CRSE_HIST_FL`) and grades-by-term
  (`SSR_VWGD_GRADE_FL`) were captured too (`captures/laccd/154_*`, `178_*`)
  but are unused: the PDF is reconciled against its own totals, those pages
  are not.

## Mistakes already made once

- 2026-10-06/07: a coverage pass rewrote multipart parts from `mime_str`
  to `.headers(HeaderMap)`, which sends the part's `Content-Type` lowercased;
  the browser captures have `Content-Type:`. Slate's upload wire test caught
  it, the license `model` part's test checked only the part name and did
  not. Every multipart part now uses `mime_str` and its wire test pins the
  header line. Refactors "for coverage" must not change bytes on the wire.

- 2026-10-07: `just check` hung for hours (three runs stuck since the night
  before). wiremock's `MockServer::drop` runs `verify()` with
  `futures::executor::block_on` on the test's runtime thread; once a long
  test has spent tokio's coop budget, the lock's wake is deferred to that
  runtime (`context::defer`), which can't run while Drop blocks it, so the
  thread parks forever (sampled: parked in `MockServer::verify`). Every
  `#[tokio::test]` body is wrapped in `tokio::task::unconstrained` so drops
  never hit an exhausted budget. A hang in a test: `sample <pid>` before
  theorising.

- 2026-10-06, UCLA: the application was reported "filled" from the Review
  page plus blank-question listings. Neither sees list sections: Review
  doesn't require Professional Experience, so 18 paid roles in the profile
  and 0 on the portal went unnoticed until the applicant asked. Rule:
  completeness is `admit -p <portal> audit`'s per-section lines
  (`slate::coverage`): every menu section gets one, an unreadable section is
  UNCHECKED (violation), an empty list is TODO unless `[excluded]` gives a
  reason, and every paid role and college in the profile must be declared
  or excluded. `apply` prints what is still open after every run. Never
  call an application complete without that output showing nothing open
  beyond what is named.

- The first Slate `apply` (2026-10-06) posted only the one changed select
  on Biographical Data, on the untested belief that Slate keeps fields a
  post omits; it blanked the page's ten other answers. The per-page
  re-read stopped the run and the pulled answers file restored them with
  the next apply. Saves now post the whole page.
- A Slate POST without `Origin` was read as an expired session; see the
  Slate facts.

- 2026-10-10, Parchment: the sign-in page's body was not in the HARs, so
  its fixture was written as Keycloak's standard `<form>`. Every test
  passed; the first live `admit parchment login` failed before sending
  the password: the real page has no `<form>` (a React `LoginForm` posting
  to `commonProps.url.loginAction`), and the refusal reader then printed
  the inline script, which names `kc-feedback-text` as a class. The fixture
  is now the live page fetched signed out, and Keycloak messages are read
  outside `<script>`. A page that was never seen gets fetched (signed out
  is enough for a sign-in page) before a fixture is written for it.

- Four portal title typos (GIFTEDNEDD, ILLIMINATING, SUPERHEROS, MEDECINE)
  sat in plan output as "title differs" Notes for hours because a title
  difference was assumed to be an abbreviation. Now `coursework::title_typos`
  flags a portal word within a fifth of its length in edits of a transcript
  word as a Violation and plans a word-level fix. Not a typo: either word a
  prefix of the other (PSYCHOLOG/PSYCHOLOGY), or the transcript word the
  portal's minus letters ("SKLLS" for SKILLS - the first live run "corrected"
  SKILLS into SKLLS until that rule existed). Free text is checked by
  `validate/text.rs` (doubled punctuation, repeated words); a dictionary scan
  of the profile found only names and jargon, so none is built in.

- Two saves first sent the stored record back instead of the form's fields,
  and NursingCAS answered "one or more required questions are not answered"
  (2026-10-06): the high-school PUT lacked `termTypeId` and
  `graduated`/`monthGraduatedId`/`yearGraduatedId` (the stored row had them
  null), and the college PUT hard-coded `isDegree`/`degreeObtained` "yes" for
  colleges with no degrees. Build bodies from the bundle's form code, not
  the GET shape; a 4xx with that message means a form field is missing.
- `apply --only X` was blocked by profile violations in other sections
  (overlapping experience hours blocked a course-code fix); the check is now
  scoped to the sections being written (`commands::portal::scoped`).

- Website role matching first scored titles by the share of the *shorter*
  title's words found in the longer one; a long title containing every word
  of a short one scored 1.0 and won ("Summer Science Research Institute
  Bioinformatics and Computational Biology Researcher" for the site's
  "Computational Biology & Bioinformatics Researcher"). Jaccard similarity
  plus a same-start-month tiebreak replaced it; the decoy is in the test.

- The schedule first used launchd `StartInterval` for "every 8 weeks". Its
  countdown restarts at every load (login/reboot) and it doesn't run while
  asleep, so on a laptop it would never fire. launchd now runs
  `sync --if-due` daily and `sync::due` decides from `reports/last-sync`.
- Tag scanning stopped at the first `>`, truncating quoted attribute values
  that contain one (`value='<redacted>'`); `html::tag_end` honours quotes.

- LACCD course lines: matching the college column as "any capitals" turned
  `CH DEV 001 LAMC` into course `CH` at college `DEV` and silently dropped it
  (also `POL SCI 019`). The parser now accepts only known abbreviations and
  reconciles every term against the transcript's own unit totals.
- An empty plan is only evidence if the diff can see changes: after the first
  `pull`, a plan was 0 changes; a deliberate one-field edit then showed
  exactly that field. Repeat that check after changing `from_live`/payload code.

## Coverage findings

- Slate arithmetic formulas may contain multibyte Unicode whitespace. Advance
  by `char::len_utf8()` when consuming it; slicing at byte 1 panics for an
  em space. `answer::formula_boundaries` pins arithmetic and malformed tokens.
- llvm-cov's per-file region summary credits each function's *best single
  instantiation*, not the union. The CLI is compiled twice (unit-test binary
  and the `admit` binary `crates/cli/tests` runs), so 22 regions that ran
  read as uncovered. `scripts/coverage.py` unions regions over
  instantiations; lines and functions still come from llvm-cov.
- An error test that only checks the message ("HTTP 500") can pass on an
  earlier call failing the same way: Slate `findings` reads a page in
  `coverage()` and `pages()` before `plan_school_details` does. Pin which
  call failed (request count), and treat an uncovered `?` under a test that
  claims it as a broken test.
- The line metric has the same flaw: llvm-cov counts a file's lines from
  each function's best instantiation, so a branch only a unit test reaches
  (the `--lib` build) can read as uncovered when the integration-test build
  of the same function covers more overall (2026-10-07: `page()`'s skip of
  a nested question marker). Reach such a branch from `crates/<c>/tests/`.
  Adding integration tests can *cause* this: on 2026-10-07 new
  `tests/documents.rs` cases made the integration build of `documents::plan`
  the credited one, and three branches only its unit test reached went
  uncovered. A generic fn has the same problem per caller (`Lock::update`
  was split into non-generic load/save for it).
- The first `just fuzz` run (2026-10-07, 2 min per target) found three Slate slicing panics, all one shape: a closer
  searched for from before the opener (`part.find("</select>")` instead of
  `part[s..]`), or `i - 1` into a multibyte char. Search for a closer from
  the end of its opening tag. Transcript, LACCD, website and html readers
  survived ~250k-875k inputs each. A useful property for Slate: `to_toml`
  of a page, read back and planned against that page, plans nothing.
- `toml::Value::String(s).to_string()` is not a key encoder: a string with
  a newline renders as `"""…"""`, illegal as a key. `answer::toml_key`.
- A fake `security` that parses `-w VALUE` with `shift 2` loops forever on a
  lookup, where `-w` is last (`shift 2` with one argument left fails
  without shifting): the transcripts tests hung in `session::get` until a
  `sample` showed it. Shift one argument at a time.
- Code that would hit a real host (`admit laccd` with stored credentials)
  is tested in a child process whose `HTTPS_PROXY` is a local listener
  recording the CONNECT line; no endpoint override is added for tests.
