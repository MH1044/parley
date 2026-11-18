import { describe, expect, it } from 'vitest'
import type { Exercise } from '../../api/commands'
import { EMPTY_DRAFT, answerOf, choosePair, meaningOrder, pairKey, usesWordBank } from './draft'

const base = { id: 'e1', item_id: 'es.a1.u01.s01#l:hola', facet: 'receptive', origin: 'template' } as const

const pick: Exercise = {
  ...base,
  type: 'pick_meaning',
  prompt_target: 'hola',
  options: ['hello', 'night', 'day', 'good'],
  answer_index: 0,
}

const translate: Exercise = {
  ...base,
  facet: 'productive',
  type: 'translate_to_target',
  prompt_en: 'Good morning.',
  target: 'Buenos días.',
  accepted: ['Buenos días.'],
  tiles: ['días', 'noches', 'Buenos'],
}

const pairs: Exercise = {
  ...base,
  type: 'match_pairs',
  pairs: [
    ['hola', 'hello'],
    ['adiós', 'goodbye'],
  ],
}

describe('the answer a draft gives', () => {
  it('needs a choice, every pair, or some text', () => {
    expect(answerOf(pick, EMPTY_DRAFT, false)).toBeNull()
    expect(answerOf(pick, { ...EMPTY_DRAFT, choice: 1 }, false)).toEqual({ choice: 1 })
    expect(answerOf(pairs, { ...EMPTY_DRAFT, pairs: [[0, 0]] }, false)).toBeNull()
    expect(
      answerOf(pairs, { ...EMPTY_DRAFT, pairs: [[0, 0], [1, 1]] }, false),
    ).toEqual({ pairs: [[0, 0], [1, 1]] })
    expect(answerOf(translate, { ...EMPTY_DRAFT, text: '   ' }, false)).toBeNull()
    expect(answerOf(translate, { ...EMPTY_DRAFT, text: 'Buenos días' }, false)).toEqual({
      text: 'Buenos días',
    })
  })

  it('reads word-bank tiles in the order chosen', () => {
    expect(usesWordBank(translate, true)).toBe(true)
    expect(usesWordBank(translate, false)).toBe(false)
    expect(usesWordBank(pick, true)).toBe(false)
    expect(answerOf(translate, { ...EMPTY_DRAFT, tiles: [2, 0] }, true)).toEqual({
      text: 'Buenos días',
    })
    expect(answerOf(translate, { ...EMPTY_DRAFT, text: 'typed' }, true)).toBeNull()
  })
})

describe('making pairs (match_pairs)', () => {
  const word = (index: number) => ({ side: 'word', index }) as const
  const meaning = (index: number) => ({ side: 'meaning', index }) as const

  it('a half is chosen, let go when chosen again, and replaced by another of its side', () => {
    const half = choosePair(EMPTY_DRAFT, word(0))
    expect(half).toEqual({ ...EMPTY_DRAFT, half: word(0) })
    expect(choosePair(half, word(0))).toEqual(EMPTY_DRAFT)
    expect(choosePair(half, word(2))).toEqual({ ...EMPTY_DRAFT, half: word(2) })
  })

  it('the other side makes the pair, a word then a meaning or a meaning then a word', () => {
    const made = choosePair(choosePair(EMPTY_DRAFT, word(0)), meaning(1))
    expect(made).toEqual({ ...EMPTY_DRAFT, pairs: [[0, 1]] })
    expect(choosePair(choosePair(EMPTY_DRAFT, meaning(1)), word(0))).toEqual(made)
  })

  it('a side already in a pair cannot be chosen', () => {
    const made = { ...EMPTY_DRAFT, pairs: [[0, 1]] as [number, number][] }
    expect(choosePair(made, word(0))).toEqual(made)
    expect(choosePair(made, meaning(1))).toEqual(made)
    const half = { ...made, half: word(2) }
    expect(choosePair(half, meaning(1))).toEqual(half)
  })

  it('a number chooses a word and a letter the meaning shown on its row', () => {
    const order = [2, 0, 3, 1]
    expect(pairKey('1', 4, order)).toEqual(word(0))
    expect(pairKey('4', 4, order)).toEqual(word(3))
    expect(pairKey('5', 4, order)).toBeNull()
    expect(pairKey('0', 4, order)).toBeNull()
    expect(pairKey('a', 4, order)).toEqual(meaning(2))
    expect(pairKey('B', 4, order)).toEqual(meaning(0))
    expect(pairKey('d', 4, order)).toEqual(meaning(1))
    expect(pairKey('e', 4, order)).toBeNull()
    expect(pairKey('Enter', 4, order)).toBeNull()
  })

  it('the meanings keep the order given, and never show in the order of the words', () => {
    expect(meaningOrder(4, [2, 0, 3, 1])).toEqual([2, 0, 3, 1])
    for (const broken of [undefined, [0, 1, 2, 3], [1, 0, 2], [1, 1, 2, 3], [4, 0, 1, 2]]) {
      const order = meaningOrder(4, broken)
      expect([...order].sort(), `${broken}`).toEqual([0, 1, 2, 3])
      expect(order, `${broken}`).not.toEqual([0, 1, 2, 3])
    }
  })
})
