# What admit supports

Every write below is one of four levels, and only the first two are
evidence that the body is right:

- **live**: admit ran it against the real portal and the re-plan or the
  portal confirmed it (date given).
- **captured**: the browser's request is in `captures/` (2026-10-06/07);
  *pinned* where a wire test holds admit's body to it. admit's own run has
  not been observed.
- **bundle**: built from the applicant JavaScript bundle; no browser request
  was ever captured. Treat the first live run as the test.
- **no**: not automated. Do it in the browser.

## Deployments exercised

| Kind | Deployment | Used for | Evidence |
| --- | --- | --- | --- |
| Liaison CAS | NursingCAS 2027 cycle, `nursingcas2027.cas.myliaison.com` | Everything in the Liaison table | Live since 2026-10-06; responses recorded in `shapes/nursingcas.json` |
| Slate | UCLA Graduate Division, `apply.grad.ucla.edu`, applications 2027-2028 and 2026-2027 (`same_as`) | Read, plan, write | Live 2026-10-06/07 |
| Slate | Azusa Pacific, `engage.apu.edu`, **after submission** | Status checklists and the update letter (read only) | Captured 2026-10-06; fixtures `apu_status.html`, `apu_update.html` |
| LACCD PeopleSoft | `csprd.laccd.edu` | Sign-in and unofficial transcript download | Live; flow in `captures/laccd/flow.json` |
| Website | schema.org JSON-LD on `[website] url` | Profile comparison (read only) | Live |
| Clearinghouse | `tsorder.studentclearinghouse.org` | Transcript order status (read only) | Live 2026-10-10 (order 97852158) |
| Parchment | `www.parchment.com/lp` | Transcript order list (read only), sign-in | Order list live 2026-10-10 on a browser session; sign-in captured only |

**No other deployment has been exercised.** Nothing in the adapters names a
CAS, cycle or school (only `base_url` does), but that only means nothing is
known to break. Before writing to another Liaison CAS or another cycle:

1. Add it to `admit.toml` with its own `base_url`.
2. `cp shapes/nursingcas.json shapes/<name>.json`, then `admit -p <name> drift`.
   Each Violation is a response the readers would misread on that portal.
   Zero Violations means its responses are shaped like the ones the readers
   were built and verified on.
3. Only then `admit -p <name> drift --record`. `apply` refuses to write to a
   portal that has no recording (`drift::before_writing`).

A program set with features this one lacks (question kinds other than
radio, dropdown and textarea; documents other than CV, résumé and
authorization) reads, but its writes are untested.

A Slate instance other than UCLA reads its pages generically
(`form_question`, `data-export`), but answers are keyed by that school's
export keys, and its own audit (Review page, coverage) is the only check.

## Liaison (NursingCAS 2027)

| Section | Read | Create | Edit | Delete |
| --- | --- | --- | --- | --- |
| achievements | live | captured, not pinned | bundle (no PUT captured) | no (captured, never called) |
| licenses | live | captured, pinned (multipart `model` part header) | bundle (the same POST with the existing id) | no (captured, never called) |
| experiences | live | captured, not pinned | bundle (PUT with every block; wire test pins the resend) | no (captured, never called) |
| education (degrees, still attending) | live | captured, pinned (add institution: the body the form sent for Pierce) | captured (`PUT_academicHistory_colleges_n`) | no |
| coursework | live | courses: captured; new terms: bundle | **live** 2026-10-06, course PUT (four title typos fixed; 2026-10-07 re-plan clean) | no |
| high-school | live | captured (diploma POST) | **live** 2026-10-06, attendance PUT | no |
| prerequisites | live | captured, pinned (16 posts replay byte for byte) | captured, pinned | no |
| questions | live | captured, pinned (6 saves replay; radio, dropdown, textarea only) | captured, pinned | no |
| documents | live (content too: `view`, 2026-10-08) | **live** 2026-10-07 (CV uploads) | captured, pinned (replace = save the held file, delete, upload; on failure re-upload the saved file) | captured (only inside a replace) |
| references | live | bundle (each request emails a person; needs `consent = true`) | no | no |

Also: marking Documents/Questions done is **live** (2026-10-07: Yale, JHU
×2, Rush). Posting grades into an in-progress term is **no**: the shape is
in the bundle but has never been seen to succeed, so the plan reports it
as Pending. Personal sections (Biographic, Contact) are read and compared,
never written. Program-level `orgQuestions` are not read.

## Slate (UCLA)

| Operation | Level |
| --- | --- |
| Question pages (whole-page multipart save) | live 2026-10-06 (posts every field: Slate clears omitted ones) |
| Addresses: insert | live 2026-10-06 (the mailing address) |
| Addresses: edit a row | no (reported as differences) |
| Academic History: add institution + transcript | live 2026-10-06 |
| Additional Academic Info (major, GPA, scale, dates) | live 2026-10-06 (12 institutions) |
| Professional Experience: add employer | live 2026-10-06 |
| Uploads (`material` questions), tests, signature | no (pages with uploads are refused) |
| Choosing the application (`.application` cookie) | live |
| Review page errors and warnings, section coverage | live (read) |

## Transcript orders (`admit transcripts`)

| Operation | Clearinghouse | Parchment |
| --- | --- | --- |
| Read order status | **live** 2026-10-10 (one order, four statuses seen) | **live** 2026-10-10 (5 orders, all `COMPLETE`) |
| Sign in | none needed (order number + email) | captured, pinned (password, emailed code, trust device); admit's own sign-in not yet run |
| Place an order | no | no |

Placing orders is deliberately not automated. A Clearinghouse order sends
the SSN, a card (Clover tokenises it in the browser; the captured request
holds the CVV in clear), and a drawn release signature. A Parchment order
pays on Stripe Checkout, and nothing after the payment page was captured.
Order in the browser, then record a Clearinghouse order number in
`profile/transcript_orders.toml`.

## Safety rails that apply everywhere

- `apply` refuses on profile violations in the sections it writes, and
  fails unless the re-plan shows no changes and no violations (`ctx::settle`).
- Before the first write it compares the responses those sections are built
  from with the recording, and stops on any change.
- Each write is journalled in `reports/journal-<portal>.log` before it is
  sent. A document replace saves the held file to
  `profile/documents.replaced/` and records itself in
  `profile/documents.lock.json` before its DELETE, and uploads the saved
  file again if the new one is refused.
- Nothing outside the profile is deleted. The only DELETE is a document
  replace.

## Known limits

- A replace still deletes before it uploads: a slot holds one attachment,
  and uploading into a full slot has never been tried. Between the two the
  slot is empty; the restore narrows that window, it does not close it. A
  restore re-uploads the same bytes, as a new attachment id with today's
  upload date.
- Parchment's trusted device lasts a day (`Max-Age=86400`), and its code
  goes to the account's email, so an unattended run can sign in only within
  a day of `admit parchment login`; CI does not read Parchment.
- Parchment: only the first page of orders is read (`onLoadMore` was never
  captured). Clearinghouse: only status code `ETS` is known to mean sent;
  other codes are shown as open.
- Other Liaison deployments and cycles are untested (above). Operations
  marked *bundle* have never been seen on the wire.

`crates/cli/tests/support_doc.rs` fails if a Liaison section is missing
from the table above.
