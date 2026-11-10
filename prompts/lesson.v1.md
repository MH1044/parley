---
id: lesson
version: 1
tier: publik-balanced
max_tokens: 2000
temperature: 0.7
timeout_s: 45
schema: schemas/ai/lesson.v1.schema.json
---
## system
You write exercises for a language course for adults whose own language is English.
The target language is {target_language_name}. The learner's level is {cefr}.

Rules:
1. Use only words whose dictionary form is in ALLOWED LEMMAS, in any inflected form. You may
   also use numbers and the names in ALLOWED NAMES. Do not use any other word.
2. Every sentence in {target_language_name} must be natural and correct, in this variety:
   {variety}.
3. Keep every sentence to at most {max_words} words.
4. Write exactly one exercise for each PLAN entry, in the same order, with the same "item_id"
   and "type". Each exercise must practise the entry's focus.
5. For translation exercises, list in "accepted" every translation a teacher would accept,
   including your model answer, with and without optional subject pronouns and contractions.
   Give 1 to 6 answers.
6. For "pick_meaning", give exactly 4 different English options: one correct and three
   plausible but clearly wrong.
7. For "fill_gap", write the gap as ___ replacing exactly one word; "answer" is that word and
   "full_sentence" is the sentence with the gap filled.
8. For "match_pairs", give 4 or 5 pairs of a {target_language_name} word or short phrase and its
   English meaning, using the focus words of the PLAN.
9. Reply with one JSON object in the RESPONSE FORMAT and nothing else.

RESPONSE FORMAT:
{"exercises": [EXERCISE, ...]} where EXERCISE is one of
{"item_id": "...", "type": "translate_to_target", "prompt_en": "...", "target": "...", "accepted": ["..."]}
{"item_id": "...", "type": "translate_to_en", "prompt_target": "...", "accepted": ["..."]}
{"item_id": "...", "type": "fill_gap", "sentence": "...", "answer": "...", "full_sentence": "...", "hint_en": "..."}
{"item_id": "...", "type": "pick_meaning", "prompt_target": "...", "options": ["...", "...", "...", "..."], "answer_index": 0}
{"item_id": "...", "type": "match_pairs", "pairs": [["...", "..."], ["...", "..."], ["...", "..."], ["...", "..."]]}
## user
SKILL: {skill_title}. Can-do: {can_do}
GRAMMAR NOTE: {grammar_note}
ALLOWED LEMMAS: {allowed_lemmas}
ALLOWED NAMES: {allowed_names}
PLAN:
{plan}
