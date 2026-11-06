# Seed course (frozen)

The course Parley bundled before the full A1 and A2 syllabi: Spanish A1 units 1 and 2 (8
skills), French and German unit 1 (4 skills each), each with `syllabus.yaml`, `forms.tsv` and
`numbers.yaml`, copied unchanged from `content/<lang>/` as module 01 wrote them.

It is kept here because the module 04 command tests
(`app/src-tauri/tests/m04_app_shell_and_offline_course.rs`) work out their expected values by
hand over this course: the placement staircase over its eight Spanish skills, the 76 vocabulary
items behind the stats, and the item ids the hand-written backups in
`tests/cases/04-app-shell-and-offline-course/` refer to. The app itself bundles `content/`.

Never edit these files to follow `content/`: they are a reference, not a copy to keep in step.
Unlike the other fixtures, which are CC0, these files are course content and keep the licence of
what they were copied from, CC BY-SA 4.0 (`content/LICENSE`). They hold no third-party data: the
syllabi and the forms tables were written by hand for Parley, not derived from Wiktionary.
