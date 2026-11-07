# Chat-style feed: progress and handoff

Plan: `docs/superpowers/plans/2026-11-06-chat-feed.md`
Spec: `docs/superpowers/specs/2026-11-06-chat-feed-design.md` (approved by the owner)
Branch: `feat/chat-feed`
Execution: native (executing-plans skill), chosen by the owner.

## Where it stopped (2026-11-07)

- Task 1, Step 2 is done: the failing tests are written in `src/feed/entry.rs`
  (no implementation yet), `src/feed/mod.rs` declares `pub mod entry;`, and `src/main.rs`
  has `pub mod feed;`. `cargo build` should still pass, because the tests are `cfg(test)`.
- Task 1, Step 1 (baseline `cargo test`) has not completed: the toolchain was downloaded,
  but the run was stopped while compiling.
- **Next:** on the new machine, rerun Task 1 Step 1, then Step 3 (watch the tests fail),
  Step 4 (implement), and continue the plan.

## Toolchain notes

- The original machine had no cargo and no nixpkgs channel; `nix-shell -p ...` (the plan's `RUN`)
  failed with "file 'nixpkgs' was not found". What worked was the flake form:

  ```sh
  nix --extra-experimental-features 'nix-command flakes' shell \
    nixpkgs#cargo nixpkgs#rustc nixpkgs#gcc nixpkgs#gawk nixpkgs#gnumake \
    nixpkgs#perl nixpkgs#pkg-config nixpkgs#clippy -c sh -c '<cmd>'
  ```

  (cargo 1.98 / rustc 1.98.1.) On a machine with a normal Rust toolchain, just run `cargo` directly.
- The superpowers `task-start` script needs `awk`; it was missing on the original machine.
- Keep `.superpowers/` out of git (`echo /.superpowers/ >> .git/info/exclude`); Task 6 adds it to
  `.gitignore`.

## Ledger (copy of `.superpowers/sdd/2026-11-06-chat-feed/progress.md`)

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

No task is complete yet.
