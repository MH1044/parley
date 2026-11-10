# Replayed P1 requests (gate G1, scaled down)

Each file is one fresh-lesson request and its answer: `prompt` (the `LessonPrompt` sent), `plan`
(the composer's plan it was made from) and `content` (the answer as the call returned it, once it
passed its schema). The normal
test suite replays every file here through the lesson validators and expects at least 80% of the
planned exercises to pass, the threshold of gate G1 (`scripts/gates/thresholds.toml`, `[g1]`).

The three files here now are hand-written stand-ins for the first skill of each course, written to
follow the prompt's rules. The first live run of G1 (`scripts/gates/run --live G1`, the
maintainer's) writes real answers here in the same shape, which then replace them.
