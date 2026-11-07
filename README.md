# MicroBin feed

A personal, self-hosted feed for moving things between your own devices: paste, drop or tap + to
add text, links, images and files, and they appear on every open screen within a couple of seconds.

This is a fork of [MicroBin](https://github.com/szabodanika/microbin) rebuilt around a single
chat-style page. It has no accounts, sharing levels, encryption or expiry by default, and it makes
no outgoing network requests. It is meant to run behind your own network or VPN, which is what
controls access.

## Features

- One page: the newest entries at the bottom, older ones load as you scroll up
- Text is rendered as sanitized markdown; Copy always gives the raw text
- Screenshots and images paste straight from the clipboard, files drag and drop
- Live updates across devices, including catching up after a phone has slept
- Search over text, links and file names
- QR code and short link for any entry
- Optional per-entry expiry (1 hour, 1 day, 1 week) and a default on the settings page

## Run it

```bash
docker compose up --build --detach
```

The feed is then on port 8080. Data lives in the `microbin_data` volume: `feed.sqlite` and the
`feed_files/` directory. Data from an older MicroBin version is left alone and is not imported.

Or run it directly with `cargo run --release -- --data-dir ./microbin_data`.

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `MICROBIN_PORT` | `8080` | Port to listen on |
| `MICROBIN_BIND` | `0.0.0.0` | Address to listen on |
| `MICROBIN_DATA_DIR` | `microbin_data` | Where the database and files are stored |
| `MICROBIN_TITLE` | `microbin` | Name shown in the header |
| `MICROBIN_THREADS` | `1` | Worker threads |
| `MICROBIN_MAX_FILE_SIZE_MB` | `2048` | Largest file you can upload |
| `MICROBIN_BASIC_AUTH_USERNAME` | not set | Turns on HTTP basic auth when set |
| `MICROBIN_BASIC_AUTH_PASSWORD` | not set | Password for basic auth |

The basic auth values can also read from a file, as `file:/path/to/secret`.

Available under the [BSD 3-Clause License](LICENSE).
