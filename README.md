# admit

Keep nursing-school applications in line with one declared profile.

```
just validate        # offline: limits, dates, overlapping hours
just pull            # live application -> profile/*.toml (first run)
just plan            # what would change on the portal
just apply --yes     # change it, then re-plan to confirm
just audit           # every portal: deadlines soonest first, what's still open
admit -p ucla audit  # one portal in full, notes included
admit website        # profile vs your website's structured data
admit transcript FILE.pdf --parser laccd|elmbridge|conncoll
admit laccd fetch           # sign in to LACCD, download LAC_SR_TSRPT.pdf
admit sync                  # download LACCD transcript -> NursingCAS coursework
admit schedule install      # daily `sync --if-due`: term ends + every 8 weeks
```

## Setup

Start from `examples/`: copy `examples/admit.toml` and `examples/profile/` (a
fictional applicant) to the repository root, where `admit.toml` and `profile/`
are git-ignored so your own data stays out of the repository.

1. Log in to NursingCAS, open DevTools → Network, click any request to
   `/applicant/api/`, copy its `Cookie` request header, then
   `pbpaste | admit session set` (stored in the macOS Keychain; sessions last
   hours). `admit session check` tells you when it has expired.
2. `just pull` once to seed `profile/`. From then on edit the TOML, `just plan`,
   `just apply --yes`.
3. Coursework comes from `profile/coursework.toml`: each `[[transcript]]` is
   parsed and courses missing from the portal are added, creating terms as
   needed. Choose a portal subject for any department the portal hasn't seen.

## Profile files

| File | Matches portal records by |
|---|---|
| `achievements.toml` | name |
| `licenses.toml` | title |
| `experiences.toml` | organization / title / start |
| `education.toml` | college name, then degree kind / major |
| `coursework.toml` | transcript course code or title, per college |

`ids = { nursingcas = 123 }` (written by `pull`) lets you rename an entry
without creating a new one. Entries on the portal but not in the profile are
reported and left alone.

## Checks

`admit validate` (offline): field limits, dates, degree status vs date,
overlapping weekly hours, licenses expired or expiring within 90 days, a
degree dated before the high-school diploma.

`admit website`: every role on the site exists in `experiences.toml` with the
same start/end month; every credential on the site exists in
`licenses.toml` (matched by credential ID) with the same expiry month.

`admit audit`: both of the above, plus from the portal — incomplete
sections, each program's deadline (Pending within 30 days, Violation once
passed), recommendations requested/received vs the minimum, unpaid programs,
official transcripts not ordered/received, unmatched prerequisites, and
"still attending" flags that disagree with in-progress terms. Without
`-p` it audits every `[[portal]]`: one list of deadlines (a Slate portal
declares its own with `deadline = "YYYY-MM-DD"`), then each portal's
violations and pending items; a portal it can't read is UNCHECKED.

## GitHub Actions

`.github/workflows/sync.yml` runs `admit sync` weekly and on the term-end
dates without the laptop. It needs four repo secrets; from the Keychain:

```
for a in laccd-user:LACCD_USER laccd-password:LACCD_PASSWORD \
         nursingcas-user:NURSINGCAS_USER nursingcas-password:NURSINGCAS_PASSWORD; do
  security find-generic-password -s admit -a "${a%%:*}" -w | gh secret set "${a##*:}" -R <you>/admit
done
```

Course-code corrections are never applied there; run `admit plan` / `admit
apply --yes` for those.

## Portals

- `kind = "liaison"`: every Liaison CAS (NursingCAS, PharmCAS, …). Add a
  `[[portal]]` per CAS and cycle; `--portal NAME` picks one.

- `kind = "slate"`: a Slate (Technolutions) application, e.g. UCLA Graduate
  Division (`-p ucla`). Its declared state is `profile/slate/<name>.toml`, a
  table of question answers: `admit -p ucla pull` writes what the portal has,
  `plan` / `apply --yes` push edits, `audit` lists the portal's own Review
  errors. `printf '%s\n%s\n' EMAIL PASSWORD | admit -p ucla session login` keeps
  the sign-in in the Keychain; sessions expire after minutes idle and are
  renewed by signing in again. `application = "<id>"` pins which of the
  account's applications is edited (`admit -p ucla session check` lists them).
  `[[addresses]]` in the same file are inserted through the address widget.
  A submitted Slate application (`submitted = true`, e.g. `-p apu`) is only
  read: `admit -p apu status` lists its checklists and saves the latest
  update letter to `reports/`; `audit` reports what is still awaited.

Other portals need their own adapter crate; see `AGENTS.md`.

## Scheduled sync

`printf '%s\n%s\n' 'escsso\STUDENTID' 'PASSWORD' | admit laccd login` stores the
LACCD sign-in in the Keychain (`admit laccd fetch` downloads the transcript by
hand). `admit schedule install` registers a launchd job (`com.admit.sync`)
that runs `admit sync --if-due` daily at 09:00; it is due on each `run_on`
date (a week after each LACCD term ends) and when `every_weeks` have passed
since the last successful sync. Each run downloads the LACCD unofficial
transcript (falling back to the newest inbox PDF), installs it only if it
parses, reconciles, and changes something (a grade posted, a class added or
dropped — listed in the report), adds new courses, terms and LACCD colleges
to NursingCAS, and writes `reports/sync-<date>.txt` plus a notification.
Posted grades and conflicts are reported for you to enter. NursingCAS signs
in again by itself once `printf '%s\n%s\n' EMAIL PASSWORD | admit session login`
has stored the portal sign-in; if sign-in fails the run says so and retries
the next day.

## License

[AGPL-3.0-or-later](LICENSE).
