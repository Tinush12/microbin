# Chat-style feed: verification checklist

Plan: `2026-11-06-chat-feed.md`, Task 11.

## Automated (2026-11-07)

- `cargo test`: 56 passed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- Playwright against the release binary: all specs passed.
- Live server: 6 MB upload accepted; 30 MB rejected with the file name and limit in a 413 over a real
  socket, with no temp files left and no entries created; basic auth gives 401 without credentials.

## Not verified

- [ ] `docker build -t microbin-feed .` and a run with a volume. Docker is not installed on the machine
  this was built on. Nothing in the `Dockerfile` changed, but the image has not been built.

## Owner checks (not done yet)

Record each answer here.

- [ ] PC, ungoogled Chromium on niri: paste a screenshot taken with the usual screenshot tool.
- [ ] PC: paste an image copied from a web page.
- [ ] PC: drop a file from the file manager.
- [ ] Phone over VPN: + → photo picker, send a screenshot.
- [ ] Phone: paste a link.
- [ ] Phone: the feed fits without sideways scrolling; the send button stays reachable above the keyboard.
- [ ] Phone → PC: the screenshot appears on the open PC tab within a few seconds.
