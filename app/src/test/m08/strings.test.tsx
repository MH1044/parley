// Module 08 strings (docs/modules/08-judging-and-explanations.md, UI and Design notes): the fixed
// texts of the result sheet's judging and Explain states, and the three messages the core sends,
// are among the strings of app/src/strings/en.json, with the placeholders written as the module 04
// strings write them ("Correct answer: {corrected}").

import { describe, expect, it } from 'vitest'
import en from '../../strings/en.json'

const strings: Record<string, string> = en

describe('Judging and Explain strings', () => {
  it('holds the fixed texts of the result sheet', () => {
    const values = new Set(Object.values(strings))
    const contract = [
      'Checking…',
      'Not checked',
      'Check with AI',
      'I was right',
      'Also correct: {expected}',
      'Explain',
      'Explaining…',
      'Tip: {tip}',
      'Try again',
    ]
    expect(contract.filter((text) => !values.has(text))).toEqual([])
  })

  it('holds the messages the core sends', () => {
    const values = new Set(Object.values(strings))
    const messages = [
      "You're offline, so Parley can't check free answers.",
      'Explanations need a connection or a local model.',
      'Explanations are available after an answer that was almost right or wrong.',
    ]
    expect(messages.filter((text) => !values.has(text))).toEqual([])
  })
})
