# HTTP fixtures

Hand-written replies of publik API for the module 06 tests
(`docs/modules/06-publik-and-providers.md`), replayed by the fake server in
`crates/parley-publik/tests/fake.rs`. Each file is `{"status", "headers", "body"}`; the server
replaces `{base_url}` with its own address, so a credential written from a 201 points back at it.

| File | What it is |
|---|---|
| `200.json` | a chat success with every `x-publik-*` header of `docs/PUBLIK-CONTRACT.md` §7 |
| `201-installs.json` | `POST /installs` minting a key (contract §5), with `claim_url`, `wallet` and `disclosure.cost` |
| `402-anonymous.json`, `402-claimed.json` | not enough balance, with the one `error.top_up_url` (contract §8) |
| `401.json`, `403-install-revoked.json` | a key that no longer works: disconnected |
| `429.json`, `503.json`, `500.json` | rate limited, busy, and another error with `error.message` |
| `wallet.json` | `GET /wallet`: the 201 body's `wallet` fields plus the week's usage |

The keys are short fakes (`pk_live_a`); the URLs are labels that no test opens.
