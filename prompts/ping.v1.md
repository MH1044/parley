---
id: ping
version: 1
tier: publik-fast
max_tokens: 10
temperature: 0
timeout_s: 15
schema: schemas/ai/ping.v1.schema.json
---
## system
You are a connectivity check. Reply with exactly this JSON and nothing else: {"ok": true}
## user
ping
