// Module 04 acceptance test 14: every t('…') key used under app/src exists in
// app/src/strings/en.json, a flat object of keys and strings, and the fixed texts of the spec's UI
// contract are among its strings. The files are read as text through Vite's glob imports.

import { describe, expect, it } from 'vitest'

/** Every .ts and .tsx file under app/src except tests and the test setup, by path. */
const SOURCES = import.meta.glob<string>(
  ['../../**/*.{ts,tsx}', '!../../**/*.test.{ts,tsx}', '!../../test/**', '!../../test-setup.ts'],
  { query: '?raw', import: 'default', eager: true },
)

const EN_JSON = import.meta.glob<string>('../../strings/en.json', { query: '?raw', import: 'default', eager: true })

function strings(): Record<string, unknown> {
  const texts = Object.values(EN_JSON)
  expect(texts, 'app/src/strings/en.json exists').toHaveLength(1)
  return JSON.parse(texts[0]) as Record<string, unknown>
}

describe('UI strings (acceptance test 14)', () => {
  it('en.json is a flat object of strings', () => {
    const table = strings()
    expect(typeof table).toBe('object')
    expect(Array.isArray(table)).toBe(false)
    for (const [key, value] of Object.entries(table)) {
      expect(typeof value, key).toBe('string')
    }
  })

  it("every t('…') key used under app/src exists in en.json", () => {
    const table = strings()
    const used = new Map<string, string>()
    for (const [file, text] of Object.entries(SOURCES)) {
      for (const match of text.matchAll(/\bt\(\s*(['"])([^'"\n]+)\1/g)) {
        used.set(match[2], file)
      }
    }
    expect(Object.keys(SOURCES).length, 'source files were read').toBeGreaterThan(0)
    expect(used.size, 'the screens take their text from t()').toBeGreaterThanOrEqual(10)
    const missing = [...used].filter(([key]) => !(key in table)).map(([key, file]) => `${key} (${file})`)
    expect(missing).toEqual([])
  })

  it('holds the fixed texts of the UI contract', () => {
    const values = new Set(Object.values(strings()))
    const contract = [
      'Welcome to Parley',
      'Learn by speaking. No hearts, no ads, nothing to buy.',
      'Start',
      'Choose a language',
      'Parley teaches from English',
      'Course pack',
      'Download',
      'Skip for now',
      'Continue',
      'Daily goal',
      'Where to start',
      'Take the placement test (about 5 minutes)',
      'Start from the beginning',
      'Placement test',
      'Your answer',
      'Next',
      "I don't know",
      'Go to my path',
      'Stats',
      'Settings',
      'Start lesson',
      'Fill the gap',
      'Check',
      'Quit this lesson?',
      'Quit',
      'Keep going',
      'Correct',
      'Almost',
      'Typo',
      'Wrong',
      'Not checked',
      'I was right',
      'Lesson complete',
      'Back to my path',
      'Finish a lesson to see your stats.',
      'Learning',
      'Data',
      'About',
      'Export a backup',
      'Restore a backup',
      'Importing replaces all your progress on this computer. Export a backup first if you want to keep it.',
      'Replace my data',
      'Cancel',
      'Type delete to confirm.',
      'Delete everything',
      'Show details',
      'Example sentences come from Tatoeba (tatoeba.org) under CC BY 2.0 FR.',
      'Word forms come from Wiktionary (wiktionary.org) under CC BY-SA 4.0.',
    ]
    expect(contract.filter((text) => !values.has(text))).toEqual([])
  })
})
