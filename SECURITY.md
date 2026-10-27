# Security

## Threat model

Parley is local-first. Everything a learner owns (progress, reviews, items, conversations,
settings) lives in one SQLite file in the app data directory, and the app is fully useful with no
account and no network. The things worth protecting, and what protects them:

| Asset | Protection |
|---|---|
| Progress and settings | Local files only; a backup is an explicit export; no server, no sync, no telemetry (ADR-0011) |
| Recordings, photos and media files | Never leave the machine except inside the one request the learner triggered, after consent (invariant I13); photos are resized and stripped of every metadata block first; recordings are deleted at the end of the session unless the learner keeps them |
| The publik API key | Minted per install, stored at the publik credential path outside the app data directory with mode 0600; never reaches the web view, a log line, a status file, a backup or a command result (invariant I1) |
| The learner's own key | The OS keychain on desktop, app-private storage on Android; never in `settings.json` or a backup |
| Money | One user action makes at most one attempt per publik API call, never an automatic retry (invariant I4); identical input is served from the cache and never paid for twice (I5); a batch of more than ten metered calls is confirmed first with its estimate |
| What the learner is taught | Every AI answer passes a JSON Schema and the feature's validators before anything is stored or shown; a failure stores nothing (I2); every generated exercise refers to a syllabus item the core supplied (I11) |

Untrusted input is treated as such: course packs and backups from other people, every AI response,
and every file the learner imports are validated against a schema before use. The web view has no
filesystem, shell, dialog or HTTP permission of its own (`app/src-tauri/capabilities/default.json`);
it sends intents to commands, and the core decides what happens. The web view never opens a URL it
made up: the only links it can open are the claim, top-up and add-credit URLs the core holds.

Out of scope: a compromised operating-system account, which can read any local file.

## Every network call the app can make

This is the complete list (`docs/ARCHITECTURE.md` §8). Nothing else contacts the network, and a
test proves that only the chosen provider is ever contacted (invariant I9). At version 0.1.0 none of
these calls exists yet; each arrives with the module named.

1. **Course pack download** (module 04): from the release URLs pinned in `content/packs.lock.toml`,
   on explicit user action, with the size shown first and the SHA-256 verified.
2. **Local speech model download** (module 10): from `models/registry.toml`, on explicit user
   action, with the size shown first and the SHA-256 verified.
3. **Update check** (module 14): one request to the public releases page, carrying nothing but the
   request and a `User-Agent` of `Parley/<version>`; off in development builds; switchable off in
   Settings.
4. **AI calls**, only after the learner has chosen a provider, and only to that provider:
   - publik API (`https://publikhq.com/api/v1`): `POST /installs` (and `/installs/revoke` on
     disconnect), `GET /wallet` on start and when Settings opens while publik API is the chosen
     provider, `POST /chat/completions`, `POST /audio/transcriptions`, `POST /audio/speech`.
     `GET /models` is made only by developers during implementation and by the maintainer's
     `scripts/publik-proof`. The built-app self-test makes one `POST /installs` only when the desktop
     app is started with `PUBLIK_SELF_TEST` set, and then exits.
   - The learner's own OpenAI-compatible base URL, with their key.
   - A local model server on `localhost` (`/v1/models`, Ollama's `/api/tags`,
     `/v1/chat/completions`), which never contacts publikhq.com.

What each AI feature sends, and what never leaves the machine, is listed feature by feature in
`docs/PUBLIK-INTEGRATION.md` §10, and the consent screen says it in one sentence before the first
AI action (never at launch or in onboarding, invariant I12).

The provisioning request carries a `device_name` field only if the learner opts in, and then a
generic label (`Windows PC`, `Mac`, `Linux PC`, `Android phone`), never the hostname.

## Keys

- The publik API key rides only in the `Authorization` header: never a query string, a command
  line, a log line or a file inside the app data directory.
- The app token in `crates/parley-publik/publik-app-token.txt` is public by design: it can only
  mint keys capped to this app.
- Keys never appear in error messages, the status object, events, or anything the interface
  receives. The test suite scans every one of them for a fake key.
- Logs (`logs/parley.log`, rotating) record command names, durations, error codes, HTTP status codes,
  request ids and cache hits. Never answers, sentences, transcripts, audio, images, file paths chosen
  by the learner, keys or balances.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository (the Security tab, "Report a
vulnerability"), so the report stays private until a fix is released. Please do not open a public
issue for a security problem.
