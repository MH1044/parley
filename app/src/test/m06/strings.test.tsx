// Module 06 acceptance test 18 (strings half): the consent text in app/src/strings/en.json is the
// spec's, word for word (docs/modules/06-publik-and-providers.md, UI; docs/PUBLIK-INTEGRATION.md
// §4), and the fixed texts of the AI screens are among its strings.

import { describe, expect, it } from 'vitest'
import en from '../../strings/en.json'

const strings: Record<string, string> = en

/** `DISCLOSURE` of the publik API reference module, word for word. */
const DISCLOSURE =
  "This app can use publik API for its AI features: no key to paste, priced per use in dollars from a publik balance that starts at $0.00. Linking this computer to a publik account gives $0.05 of free use, once. Your prompts go through publik's servers to the model; you can switch to your own key at any time."

/** The data-path sentence of docs/PUBLIK-INTEGRATION.md §4. */
const DATA_PATH =
  "Parley sends only what the AI feature you use needs through publik's servers: the lesson topic and the words you know, the exercise and your answer, your conversation turn, the sentence you just recorded, the photo you picked, or the audio of a file you chose. Your progress, reviews and other recordings stay on this computer."

describe('AI strings (acceptance test 18)', () => {
  it('ai.setup.disclosure is the reference DISCLOSURE and ai.setup.data_path the §4 sentence', () => {
    expect(strings['ai.setup.disclosure']).toBe(DISCLOSURE)
    expect(strings['ai.setup.data_path']).toBe(DATA_PATH)
  })

  it('holds the fixed texts of the AI screens', () => {
    const values = new Set(Object.values(strings))
    const contract = [
      'Set up AI',
      'Parley works without AI. Set it up for fresh lessons, answer checks and conversations.',
      'Set up AI features',
      'Everything else in Parley works without AI.',
      'Continue with publik API',
      'Use my own key',
      'Use a local model',
      'Not now',
      'Sign in with publik',
      'Add a plan or pack',
      'Continue',
      'AI',
      'publik API',
      'Your own key',
      'Local model',
      'In use',
      'Check connection',
      'Use publik API',
      'Disconnect',
      'Reconnect',
      'Base URL',
      'Key',
      'Chat model',
      'Save my key',
      'Remove my key',
      'Server address',
      'Find models',
      'Use this model',
      'Try again',
      'or use your own key or a local model in Settings',
    ]
    expect(contract.filter((text) => !values.has(text))).toEqual([])
  })
})
