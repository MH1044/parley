// What the learner has entered for the current exercise, and the answer it makes.

import type { Answer, Exercise } from '../../api/commands'

/**
 * One side of a pair, chosen and waiting for the other: a word by its index, or a meaning by the
 * index of the pair it belongs to (match_pairs).
 */
export interface HalfPair {
  side: 'word' | 'meaning'
  index: number
}

export interface Draft {
  text: string
  choice: number | null
  tiles: number[]
  /** Pairs made: [word index, index of the pair whose meaning was chosen]. */
  pairs: [number, number][]
  half: HalfPair | null
}

export const EMPTY_DRAFT: Draft = { text: '', choice: null, tiles: [], pairs: [], half: null }

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

/**
 * The draft after choosing one side of a pair: a side already in a pair changes nothing; the half
 * chosen again is let go; another of the same side takes its place; the other side makes the pair.
 */
export function choosePair(draft: Draft, choice: HalfPair): Draft {
  const column = choice.side === 'word' ? 0 : 1
  if (draft.pairs.some((pair) => pair[column] === choice.index)) return draft
  const half = draft.half
  if (half === null || half.side === choice.side) {
    const same = half !== null && half.index === choice.index
    return { ...draft, half: same ? null : choice }
  }
  const word = choice.side === 'word' ? choice.index : half.index
  const meaning = choice.side === 'meaning' ? choice.index : half.index
  return { ...draft, pairs: [...draft.pairs, [word, meaning]], half: null }
}

/**
 * The rows of the meanings column, top to bottom, as pair indexes: the order the exercise gives
 * when it is every pair once and not the words' own order; otherwise each meaning one row up from
 * its word (the first at the foot), so the two columns never line up.
 */
export function meaningOrder(count: number, given?: number[]): number[] {
  const own = Array.from({ length: count }, (_, index) => index)
  if (
    given !== undefined &&
    given.length === count &&
    [...given].sort((a, b) => a - b).every((index, at) => index === at) &&
    given.some((index, at) => index !== at)
  ) {
    return [...given]
  }
  return own.map((index) => (index + 1) % count)
}

/**
 * The side a key chooses in a match_pairs exercise of `count` pairs: 1 to the count the word with
 * that number, A to the count's letter (either case) the meaning on that row; any other key none.
 */
export function pairKey(key: string, count: number, rows: number[]): HalfPair | null {
  if (/^[1-9]$/.test(key)) {
    const index = Number(key) - 1
    return index < count ? { side: 'word', index } : null
  }
  if (/^[a-z]$/i.test(key)) {
    const row = key.toLowerCase().charCodeAt(0) - 'a'.charCodeAt(0)
    return row < count && row < rows.length ? { side: 'meaning', index: rows[row] } : null
  }
  return null
}

/** The letter of a meanings row: A for the first. */
export function rowLetter(row: number): string {
  return String.fromCharCode('A'.charCodeAt(0) + row)
}
