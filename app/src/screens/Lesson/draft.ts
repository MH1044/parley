// What the learner has entered for the current exercise, and the answer it makes.

import type { Answer, Exercise } from '../../api/commands'

export interface Draft {
  text: string
  choice: number | null
  tiles: number[]
  pairs: [number, number][]
}

export const EMPTY_DRAFT: Draft = { text: '', choice: null, tiles: [], pairs: [] }

/** Whether a translation into the target language is answered from word-bank tiles. */
export function usesWordBank(exercise: Exercise, wordBank: boolean): boolean {
  return wordBank && exercise.type === 'translate_to_target' && (exercise.tiles?.length ?? 0) > 0
}

/** The answer a draft gives, or null while there is none to check yet. */
export function answerOf(exercise: Exercise, draft: Draft, wordBank: boolean): Answer | null {
  switch (exercise.type) {
    case 'pick_meaning':
      return draft.choice === null ? null : { choice: draft.choice }
    case 'match_pairs':
      return draft.pairs.length > 0 && draft.pairs.length === exercise.pairs.length
        ? { pairs: draft.pairs }
        : null
    default: {
      const tiles = exercise.type === 'translate_to_target' ? (exercise.tiles ?? []) : []
      const text = usesWordBank(exercise, wordBank)
        ? draft.tiles.map((index) => tiles[index]).join(' ')
        : draft.text
      return text.trim() === '' ? null : { text }
    }
  }
}
