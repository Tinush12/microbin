# Chat-style feed: progress and handoff

Plan: `docs/superpowers/plans/2026-11-06-chat-feed.md`
Spec: `docs/superpowers/specs/2026-11-06-chat-feed-design.md` (approved by the owner)
Checklist: `docs/superpowers/plans/2026-11-06-chat-feed-checklist.md`
Branch: `feat/chat-feed`

## Where it stopped (2026-11-07)

All 11 plan tasks and the final review's fix pass are committed. Still open: the Docker build
(no Docker on the build machine) and the owner's device checks, both listed in the checklist.
Deferred minor findings are in the ledger below.

## Toolchain notes

Without a normal Rust toolchain, use the flake form:

```sh
nix --extra-experimental-features 'nix-command flakes' shell \
  nixpkgs#cargo nixpkgs#rustc nixpkgs#gcc nixpkgs#gawk nixpkgs#gnumake \
  nixpkgs#perl nixpkgs#pkg-config nixpkgs#clippy -c sh -c '<cmd>'
```

For `e2e/`: use nixpkgs#playwright-test, nixpkgs#playwright-driver.browsers, nixpkgs#nodejs and
nixpkgs#sqlite, with `PLAYWRIGHT_BROWSERS_PATH` set to the browsers path, `NODE_PATH` to
playwright-test's `lib/node_modules`, and `MICROBIN_BIN` to the binary under test. A machine with no
fontconfig setup also needs `FONTCONFIG_FILE` pointing at a config with a font directory, or Chromium
aborts on start.

## Ledger

Pre-flight:
- T1→T2: Entry/EntryKind/Expiry/safe_file_name consumed by Store — consistent.
- T2→T3: Entry/StoredFile consumed by render — consistent.
- T2/T3→T4: Store methods (page, changes_since, search, set_expiry, insert_*) and EntryJson — consistent.
- T4→T5: FeedState/FeedConfig; T5 adds `title` to FeedConfig — T4 tests must build FeedConfig via
  a helper so the field addition stays local. OK.
- T5→T6: main builds FeedState from Args (title, max_file_size_mb → bytes) — consistent.
- T7→T8/T9/T10: `window.Feed.upsert/remove/lastSeq/setCount` — consistent.

Rulings:
- `.superpowers/` excluded via `.git/info/exclude` (the plan adds it to `.gitignore` in Task 6) —
  keeps the ledger out of commits — cost if wrong: none.
- The owner asked to "create also the compose file" — covered by the Task 6 `compose.yaml` rewrite
  (`build: .`, only the remaining env vars, a data volume) — cost if wrong: the owner wanted a
  separate file; easy to add.
- The plan's `nix-shell -p` command does not work without a nixpkgs channel; use the flake form above
  — cost if wrong: none.

Ruling: Task 1 Step 1 baseline runs on an export of master — Step 2's tests already sit on the branch and cannot compile before Step 4 — cost if wrong: none.
Ruling: CARGO_TARGET_DIR and git safe.directory set per command via scratchpad env.sh; no global config changes — repo owned by uid 1000, session runs as root — cost if wrong: none.
Ruling: commit identity comes from env (claude <o7hn8z1oh@mozmail.com>, same as the branch history) — this machine has no git user configured; no config written — cost if wrong: author line on commits.
Task 1: complete (commits 68be3c3..9bb872b, tests: cargo test → test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
Task 2: Ruling: added 3 store tests beyond the plan's 7 (failed insert leaves nothing, delete removes the file, link text stored trimmed) and an index on entries.expires — kind_for_text accepts padded URLs that would break /u redirects; every request sweeps, and the anti-goal says the feed must not slow with history — cost if wrong: one index, a few tests.
Task 2: complete (commits 9bb872b..3274bc2, tests: cargo test → test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.26s)
Task 3: Ruling: the plan's markdown_is_sanitized asserted no "javascript:" substring on one line starting with <script>; CommonMark makes that whole line an HTML block, so the markdown link stays inert literal text and the assert failed on safe output — split into separate parsed cases that assert the stronger property, sanitizer unchanged — cost if wrong: none, each attack is still asserted.
Task 3: Ruling: added 5 render tests beyond the plan's 5 (class filtering, link rel/schemes, extension label, image/video, EntryJson) — all are interfaces the plan specifies but did not test — cost if wrong: a few tests.
Task 3: complete (commits 3274bc2..6d128a5, tests: cargo test → test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.35s)
Task 4: Ruling: the plan's `cargo test feed::api feed::upload` is invalid cargo syntax (one filter only) — run `cargo test feed::` — cost if wrong: none.
Task 4: Ruling: after a too-large file the server reads and discards the rest of the request before answering 413 — answering early can reset the connection and the browser would show "Could not reach the server" instead of the real message (spec anti-goal: silent failure) — cost if wrong: an oversized send takes as long as the full upload.
Task 4: Ruling: added MAX_TEXT_BYTES = 16 MiB, undo of already-created entries when a later file fails, and JSON errors for bad query/path/body extraction — plan says errors are JSON and "nothing is half-created" and text would otherwise buffer unbounded in memory — cost if wrong: pasted text over 16 MiB is refused with a clear error.
Task 4: complete (commits 6d128a5..a52f76f, tests: cargo test → test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s)
Task 5: Ruling: new QR page is templates/entry_qr.html, not a replaced qr.html — legacy src/endpoints/qr.rs still compiles against qr.html until Task 6 deletes both — cost if wrong: a file name.
Task 5: Ruling: /f/{id} uses actix_files::NamedFile (range requests so videos can seek), sends nosniff, and adds a CSP sandbox to html/svg/xml — plan's "inline" alone would let an uploaded file run scripts on the app's origin (Review Focus 4: hostile content) — cost if wrong: uploaded html opens without scripts; download still works.
Task 5: Ruling: POST /settings and POST /api/entries refuse a browser Origin whose host differs from the request host (403); no Origin (curl, scripts) is allowed — there is no login, so any web page open in the owner's browser could otherwise post into the feed or set the default expiry to 1 hour — cost if wrong: a reverse proxy that rewrites Host without X-Forwarded-Host gets 403 on sends.
Task 5: complete (commits a52f76f..a0340cf, tests: cargo test → test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.79s)
Task 6: Ruling: EntryKind::from_str renamed to parse — clippy -D warnings (required by this task) rejects the plan's name; matches Expiry::parse — cost if wrong: none.
Task 6: Ruling: added FeedState::open (clears feed_files/tmp at startup) and pages::fallback (HTML 404) — a crash mid-upload would otherwise leave up to 2 GB per file forever; the plan asks for a default 404 service — cost if wrong: an upload in flight during restart is lost, which it would be anyway.
Task 6: Ruling: no PayloadConfig/multipart limit is set — Multipart streams and is not subject to them; verified with a live 6 MB upload and a 30 MB rejection over a real socket (limit 20 MB) — cost if wrong: none, the per-file limit is enforced in upload.rs.
Task 6: Ruling: compose uses ${VAR:-default} for every variable and drops MICROBIN_BIND and MICROBIN_DATA_DIR — blank values from an empty env would fail to parse (threads) or blank the title; bind and data dir are fixed inside the container — cost if wrong: those two can still be set by editing compose.
Task 6: Ruling: the leftover-word grep excludes templates/assets/highlight (vendored; rg finds no matches there anyway); .env and docker-setup.sh left untouched though stale — not in the plan's file list, and .env may hold owner values — cost if wrong: owner sees obsolete vars, reported in the final message.
Task 6: complete (commits a0340cf..65bdffb, tests: cargo test → test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.81s)
Task 7: Ruling: e2e runs through nix flake packages (playwright-test, playwright-driver.browsers, nodejs, sqlite) with NODE_PATH set; helpers honour MICROBIN_BIN and default to the release binary — plan's nix-shell form needs a channel; a debug binary is used while iterating because it reads assets from disk, final verification uses release — cost if wrong: none.
Task 7: complete (commits 65bdffb..e5bd156, tests: cargo test → test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.76s)
Task 8: Ruling: pasted plain text goes into the box even when focus is elsewhere, the box is focused on load on desktop, and sending from a jumped view goes to `/` — spec goal is one paste away, and an entry sent in the middle of history would otherwise vanish from view — cost if wrong: small behaviours, easy to remove.
Task 8: complete (commits e5bd156..3685d18, tests: cargo test → test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s)
Task 9: Ruling: action code lives in templates/assets/actions.js rather than feed.js, and search in search.js — keeps each file focused (owner's file-size rule) with no build step — cost if wrong: one more script tag each.
Task 9: Ruling: placeRow leaves a row alone when its expiry is unchanged instead of always replacing it — entries are immutable except expiry; replacing would reload images and close open menus on every reported change — cost if wrong: none, expiry is the only mutable field.
Task 9: Ruling: rows use a grid instead of the mockup's absolute time column, and rows are focusable — the expiry marker overflowed onto the action buttons; display:none buttons cannot take keyboard focus, and the spec requires actions on keyboard focus — cost if wrong: visual only.
Task 9: Ruling: upload text normalizes CRLF to LF — browsers encode form newlines as CRLF, which broke "Copy always yields the raw text" — cost if wrong: a pasted file with deliberate CRLF comes back as LF in text entries (files are untouched).
Task 9: complete (commits 3685d18..c2989a5, tests: cargo test → test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.89s)
Task 10: complete (commits c2989a5..dac9375, tests: cargo test → test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.94s)
Task 11: Ruling: fenced code uses token colours in feed.css tied to the light/dark variables, and highlight.min.css is removed — the vendored light theme measured 1.56:1 in dark mode and 4.43:1 in light (spec test requires 4.5:1) — cost if wrong: a different palette from upstream's theme.
Task 11: Ruling: Docker build and owner device checks are recorded as not done in the checklist rather than treated as passing — no docker on this machine, and the checks need the owner's real devices — cost if wrong: a Dockerfile problem shows up at first build.
Task 11: complete (commits dac9375..7b17603, tests: cargo test → test result: ok. 56 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.69s)
Final review: code-reviewer subagent (opus), range 5268924..7b17603: 1 Critical, 3 Important, 9 Minor. Re-graded by effect: no label changed.
Final: fixed C1 content added during a send was destroyed — "what is added while a send is running stays in the composer", "text replaced while a send is running is kept whole" RED→GREEN, suite 59/59 unit, 51/51 e2e
Final: fixed I1 raw HTML rendered or dropped instead of shown — raw_html_is_shown_as_text, multi_line_html_keeps_its_lines RED→GREEN, suite 59/59 unit, 51/51 e2e
Final: fixed I2 a hung poll froze live updates with no offline marker — "a poll that never answers still ends in the offline marker, then recovers" RED→GREEN, suite 59/59 unit, 51/51 e2e
Final: fixed I3 long uploads gave no feedback and over-limit files uploaded fully first — "a running send says so", "a file over the limit is refused right away", feed_page_tells_the_composer_the_file_limit RED→GREEN, suite 59/59 unit, 51/51 e2e
Final: Ruling: the plan's markdown_is_sanitized and only_language_classes_survive tests assumed raw HTML is stripped; the spec says no raw HTML from entries, and a pastebin must show what was pasted, so the tests now assert it appears literally and the sanitizer is tested directly as the second layer — cost if wrong: pasted <b> shows as text rather than bold.
Final: Ruling: oversized files are refused when staged, with the server's wording, while other files stay — reviewer's I3 fix; the server's 413 remains as the real limit — cost if wrong: none, the page reads the limit from the server.
Final: Ruling: declined-to-judge items, all stand with no change: non-ASCII URL through /u (works); cross-site PUT/DELETE (preflight blocks); DNS rebinding (outside the VPN threat model); basic auth 400 on a wrong password, password-only config leaving auth off, non-constant-time compare (legacy, plan says auth stays as is); /static outside auth (embedded app assets only); remote images in markdown (now literal only for raw HTML; ![](http://...) is still fetched by the browser, not the server); clipboard with both files and text stages and pastes both (visible, removable); dropping a folder may show a misleading error (untested, rare); iOS copy fallback, keyboard overlap and lazy-image scroll need real devices (owner checks); .heic as file and HTTPS:// as text follow the plan's lists; brief ghost if a page answer crosses a delete (very narrow, next change clears it only on reload); count scan and unpruned change log (45 ms at 200,000 entries); no thumbnails (spec); /?around=abc gives JSON 400 (hand-edited URL only); Docker image unbuilt (recorded in the checklist) — cost if wrong: small, each is rare or visible.
Final: minor (deferred): M1 feed goes blank if all 50 loaded entries are deleted or expire, until reload
Final: minor (deferred): M2 a sent entry stays hidden behind open search results
Final: minor (deferred): M3 a second tap on a phone leaves the actions showing (focus keeps them open); its test checks only the class
Final: minor (deferred): M4 search misses "my notes" for my_notes.md and non-ASCII case differences, and stops at 50 with no note
Final: minor (deferred): M5 catch-up after a very long sleep is unbounded and loaded rows are never evicted
Final: minor (deferred): M6 one huge paste makes every feed open heavy until it leaves the newest 50
Final: minor (deferred): M7 uploaded bytes are not fsynced before the entry commits, so a power cut can truncate a file
Final: minor (deferred): M8 a customised MICROBIN_MAX_FILE_SIZE_UNENCRYPTED_MB is now ignored; docker-setup.sh installs upstream; release.yml passes the removed no-c-deps feature
Final: minor (deferred): M9 the hidden-tab test deletes a row that was never rendered; basic auth has no automated test
Cleanup (2026-11-07, owner: "räume alle altlasten auf"): resolved the M8 leftovers — docker-setup.sh and the unmodified upstream .env removed (compose has a default for every variable), release.yml no longer passes no-c-deps. Also removed SECURITY.md (named the upstream developer as contact), unreferenced logos, upstream Cargo metadata and the unused actix features. Ruling: logo.png and logo-square.png go although the plan's Task 6 said keep — nothing references them — cost if wrong: restore from git. Still deferred: M1-M7, M9.
