# Replayed P3 requests (gate G2, scaled down)

Each file is one judge request from the G2 set (`tests/gates/judge-set.jsonl`) and its answer as
the call returned it: `case` (the case id in the set), `request` (the `JudgeRequest` sent, copied
from the set), `content` (the reply text) and `ms` (the request time in milliseconds). The normal
test suite replays every file here through the judge's checks (J1 to J4) and the effects of its
verdict, and expects at least 9 of the 10 verdicts to agree with the case's `expected` label in the
set.

The ten files here now are hand-written stand-ins, written to follow the prompt's rules. The first
live run of G2 (`scripts/gates/run --live G2`, the maintainer's) writes real answers here in the
same shape, which then replace them; a case whose live answer failed the checks is printed by the
run and not written, so its stand-in stays.
