import { describe, expect, it } from 'vitest'
import type { Exercise } from '../../api/commands'
import { EMPTY_DRAFT, answerOf, usesWordBank } from './draft'

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
