---
id: test_echo
version: 1
tier: publik-fast
max_tokens: 50
timeout_s: 1
schema: tests/fixtures/prompts/test_echo.v1.schema.json
---
## system
You repeat text for a test. The level is {level}.
Reply with one JSON object and nothing else: {"echo": TEXT, unchanged}
## user
TEXT: {text_json}
