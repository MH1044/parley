# Test-only prompts

`test_echo.v1.md` and `test_echo.v2.md` are prompts in the format of `docs/PUBLIK-INTEGRATION.md`
§6.1, used only by the module 06 tests (`docs/modules/06-publik-and-providers.md`, acceptance tests
11, 14, 20 and 28). They are never compiled into the app.

- `test_echo.v1.md`: one plain placeholder (`{level}`) and one JSON-escaped placeholder
  (`{text_json}`); `timeout_s: 1`, so a fake server that answers after 2 seconds times out.
- `test_echo.v2.md`: the same prompt with a changed system text, so its version is bumped: a cache
  entry of version 1 must not answer it.
- Both schemas accept exactly `{"echo": "<1 to 200 characters>"}`.
