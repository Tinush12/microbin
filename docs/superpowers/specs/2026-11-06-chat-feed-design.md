# Chat-style feed: design

Date: 2026-11-06
Status: approved by owner, 2026-11-06
Mockup: https://claude.ai/artifact/MMwDYVxPB9wRAjsYaVDLyt (layout "B · Stream" is the chosen one)

## Intent

The owner used a WhatsApp group with only themselves in it as a personal pastebin: drop screenshots,
links, the odd file or markdown note from the phone, then pick them up on the PC (and the other way
round). They want that workflow, self-hosted, on their own MicroBin fork.

The single goal is **removing friction**. Opening the app should show everything already saved, and
saving something new should be one paste, drop or tap away. A clean, simple look matters more than
options.

Success: the owner stops reaching for WhatsApp. They take a screenshot on the phone, send it, and it
is already visible on the PC tab they have open.

## Context and constraints

- Runs on the owner's homelab only. It is not exposed to the internet. The phone reaches it over VPN.
  The network is the access control, so **no login** for normal use.
- Single user. No accounts, no sharing model.
- Owner's browsers: ungoogled Chromium on Linux (niri, Wayland) on the PC; a phone browser over VPN.
  Mobile layout has not been tested in the current app and must work.
- Stack stays as is: Rust, actix-web, askama templates, plain JavaScript, no frontend framework or
  build step. Deployed as one Docker container (existing `Dockerfile` / `compose.yaml`).
- **Start from an empty feed.** No migration of existing pastes is required. Storage format may change.
- This is the owner's own fork. Staying mergeable with upstream MicroBin is **not** a goal.

## Goals

1. The start page (`/`) is the feed: every entry, chat-style.
2. Text, images and other files can all be added from the start page by paste (Ctrl+V), drag and drop,
   or the + button.
3. New entries appear on every open screen without reloading.
4. Search over text, links and file names.
5. Works well on a phone and a desktop.
6. Entries never expire unless the owner chooses that for a single entry. Every entry is public
   (visible in the feed). There is no burn-after-reading and there are no privacy levels.

## Non-goals

- Captions or grouping several items into one entry.
- Link previews (no page titles, no thumbnails, no fetching).
- Editing an entry after it is sent (delete and resend instead).
- User accounts, login, multi-user.
- Importing old data.
- Staying compatible with upstream MicroBin.
- Burn-after-reading, privacy levels (unlisted, read-only, private, secret), encryption and the
  uploader password. Agreed with the owner: they mean nothing in a one-person feed behind a VPN, so
  they are removed from the code, not just hidden.

## Anti-goals (would count as failure even if it works)

- Pasting an image from the clipboard silently does nothing. This is the owner's main complaint about
  the current app (ungoogled Chromium, Linux: the attach label did not even change).
- A file is attached but the page gives no visible sign of it (current drag-and-drop behavior).
- The start page grows options, dropdowns or settings again.
- The feed gets slow or the page heavy as months of entries pile up.
- An entry silently disappears or is overwritten (for example by an ID collision). The only
  exception is an entry the owner explicitly set to expire.

## The start page

Layout as in the mockup, style **B · Stream**:

- **Header:** app name, entry count, search icon, settings gear. Nothing else.
- **Feed:** full-width rows separated by thin lines. On desktop the time sits in a left column; on a
  phone it sits above the entry. Day dividers ("Today", "Yesterday", then dates) between days.
  Newest at the bottom; the page opens scrolled to the bottom. Scrolling up loads older entries in
  batches.
- **Input bar, pinned to the bottom:** + button, a text box that grows with its content, send button.
  - Ctrl+V of an image or file, drag and drop of files anywhere on the page, and + all add the files
    to a **preview strip** above the text box: image thumbnails, file-name tiles for other files,
    each with a remove (×) control.
  - A full-page "Drop to add" overlay shows while files are dragged over the page.
  - Enter sends on desktop, Shift+Enter is a new line. On a phone, Enter is a new line and the send
    button sends.
  - Sending creates **one entry per item**: one for the text (if any), one per file.
  - Screenshots pasted from the clipboard arrive named `image.png`; give each a unique name
    (for example `screenshot-<timestamp>.png`) so several can be pasted in a row.
- **Per-entry actions**, shown on hover / keyboard focus on desktop and on tap on a phone:
  - Text: Copy (always copies the raw text), QR, Expire, Delete.
  - Link: Copy, Open, QR, Short link, Expire, Delete.
  - Image / file: Download, Open, QR, Expire, Delete.
  - **Expire** sets when this one entry is deleted (for example in 1 hour, 1 day or 1 week, or back
    to never). An entry with an expiry shows a small marker with the time left. When it expires, it
    is removed from every open feed through the live updates.

The mockup's clipboard handling (reading both `clipboardData.files` and `clipboardData.items`) was
confirmed by the owner to work in their browser, where the current app's handler does not. Use it as
the reference.

## Entry types and rendering

- **Text:** every text entry is rendered as markdown (headings, lists, links, emphasis, fenced code
  blocks). A single newline stays a line break, as in a chat. Rendered HTML must be sanitized: no raw
  HTML or scripts from entries. Copy gives the raw text.
  *Builder's call:* fenced code blocks are syntax-highlighted (language from the fence, or
  auto-detected). There is no separate syntax-highlighting setting and no second "show as code"
  mode.
- **Link:** a text entry that is only a URL is shown as a clickable link plus its domain. Nothing is
  fetched. It keeps the existing URL-shortener behavior (short link redirects to it).
- **Image:** shown inline in the feed (size-capped), click opens the full image.
  Video files may also play inline, as MicroBin already supports.
- **File:** tile with extension, file name, size, and a download action.

## Live updates

When an entry is added or deleted anywhere, every open feed reflects it within about two seconds,
without a reload. If the connection drops (phone sleeps, VPN reconnects), the page catches up on
reconnect without duplicating entries.

## Search

The search icon opens a search field. Typing filters to entries whose text, URL or file name matches.
Clicking a result jumps to that entry in the full timeline, with the surrounding entries loaded.

## Settings page

A separate page reached via the gear. Decided with the owner: it stays small.

- **Default expiry for new entries**, default "never". Stored on the server, so it is the same on
  every device. Changing it affects only entries sent afterwards.
- *Builder's call:* read-only information may be shown too (version, number of entries, storage
  used).

Nothing about expiry appears on the start page; expiry for one entry is set with that entry's
Expire action.

## Per-entry pages

Each entry keeps its own page (today `/upload/{id}`), because QR codes and short links need an
address to point to. These pages get the same clean look as the feed. QR codes and the URL shortener
stay.

## Storage

- Entry IDs must be unique. Today they are a random `u16` with no collision check, which is not
  acceptable for a log that keeps everything forever.
- The database is written one entry at a time. Today every change drops and rewrites the whole
  `pasta` table, and every entry is held in memory.
- Files stay on disk in the data directory next to the database.
- Upload size limits stay as configured today.
- *Builder's call:* the new version uses a new database file and a new
  directory for files, so existing data on a running instance is left untouched on disk and never
  mixed with or overwritten by new entries. The feed starts empty. Old QR codes and short links stop
  working, which is fine since nothing carries over.

## Left to the builder

- How live updates are delivered (server-sent events, WebSocket, polling).
- The markdown renderer and sanitizer crates.
- Batch size for loading older entries, and how search is implemented.
- Whether the JSON database backend is kept or dropped (SQLite is the default and is enough).
- Which other unused pages (guide page, admin page, the legacy `/list` page) are removed outright
  and which are just hidden, as long as nothing in Goals breaks.
- Exact visual styling, using the mockup as the direction.
