// Module 04 UI tests for leaving a screen and the screen history
// (docs/modules/04-app-shell-and-offline-course.md, UI contract and Design notes): every screen the
// path opens has Back to my path at its top and its foot, Esc on it returns to the path, and the
// mouse back and forward buttons and Alt+Left and Alt+Right walk the screens visited, in order,
// without the web view ever leaving or reloading the page. The app is rendered whole and the Tauri
// bridge is faked with the spec's result shapes.

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
// The stylesheet as text (vite.config.ts lets this one import through as text).
import STYLESHEET from '../../index.css?raw'

type Args = Record<string, unknown>

const fake = vi.hoisted(() => ({
  answers: new Map<string, (args: Record<string, unknown>) => unknown>(),
  calls: [] as { command: string; args: Record<string, unknown> }[],
}))

// Every command answers from `fake.answers`; arguments are recorded as the web view sends them.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    const given = args ?? {}
    fake.calls.push({ command, args: given })
    const answer = fake.answers.get(command)
    if (!answer) {
      return Promise.reject({ code: 'not_found', message: `No answer for ${command}.`, action: null })
    }
    return answer(given)
  }),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
  once: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}))

import App from '../../App'

// ---- the spec's result shapes ------------------------------------------------------------------

const SETTINGS = {
  schema: 'parley.settings/1',
  active_course: 'es',
  daily_goal_xp: 20,
  accent_leniency: 'lenient',
  answer_input: 'typing',
  desired_retention: 0.9,
  new_items_per_lesson: 5,
  lesson_length: 12,
  audio: {
    voice_source: 'os',
    os_voice: { es: null, fr: null, de: null },
    natural_voice: { es: null, fr: null, de: null },
    slow_speed: 0.75,
    autoplay: false,
  },
  speaking: { enabled: true, transcriber: 'provider', pass_threshold: 0.8, keep_recordings: false },
  ai: {
    provider: 'unset',
    fresh_lessons: true,
    prepare_next_lesson: true,
    stories: true,
    share_device_name: false,
    own_key: { base_url: null, chat_model: null, transcribe_model: null, speech_model: null, speech_voice: null },
    local: { base_url: 'http://localhost:11434/v1', chat_model: null },
  },
  update_check: true,
}

const PATH = {
  lang: 'es',
  units: [
    {
      id: 'es.a1.u01',
      n: 1,
      title: 'Hello',
      cefr: 'A1',
      skills: [
        {
          id: 'es.a1.u01.s01',
          title: 'Greetings',
          can_do: 'I can say hello and goodbye and ask how someone is.',
          cefr: 'A1',
          state: 'available',
          mastery: 0,
          tested_out: false,
        },
      ],
    },
  ],
  next_skill_id: 'es.a1.u01.s01',
  due: 3,
  streak: 4,
  freezes: 1,
  today_xp: 12,
  goal_xp: 20,
}

const SESSION = 's-00000000000000a1'

const PICK_1 = {
  id: 'e1',
  item_id: 'es.a1.u01.s01#l:hola',
  facet: 'receptive',
  origin: 'template',
  type: 'pick_meaning',
  prompt_target: 'hola',
  options: ['goodbye', 'hello', 'night', 'thank you'],
  answer_index: 1,
}

const PICK_2 = {
  ...PICK_1,
  id: 'e2',
  item_id: 'es.a1.u01.s01#l:adiós',
  prompt_target: 'adiós',
  options: ['night', 'thank you', 'goodbye', 'hello'],
  answer_index: 2,
}

const LESSON = {
  session_id: SESSION,
  kind: 'lesson',
  skill_id: 'es.a1.u01.s01',
  origin: 'template',
  exercises: [PICK_1, PICK_2],
  attributions: [],
  notice: null,
}

const NOTHING_DUE = {
  session_id: '',
  kind: 'review',
  skill_id: null,
  origin: 'template',
  exercises: [],
  attributions: [],
  notice: 'Nothing to review. Next review: tomorrow at 09:00.',
}

/** The size of a full course: the French course has 64 skills. */
const FULL_COURSE_SKILLS = 64

/** Stats of a learner on a full course: 30 days and one row with a meter for every skill. */
const FULL_STATS = {
  lang: 'es',
  empty: false,
  streak: 6,
  freezes: 1,
  days: Array.from({ length: 30 }, (_, i) => ({
    local_date: `2026-10-${String(i + 1).padStart(2, '0')}`,
    xp: i % 3 === 0 ? 0 : 20,
    active: i % 3 !== 0,
    frozen: false,
  })),
  week_xp: 80,
  record_week_xp: 120,
  words_known: 41,
  vocab_items: 900,
  skills: Array.from({ length: FULL_COURSE_SKILLS }, (_, i) => ({
    id: `fr.skill.${i + 1}`,
    title: `Skill ${i + 1}`,
    state: i < 10 ? 'practised' : i === 10 ? 'learning' : 'locked',
    mastery: i < 10 ? 0.8 : 0,
  })),
  cefr_estimate: 'around A1',
  recall_rate: 0.85,
  reviews_30: 120,
}

const STATUS_UNSET = {
  provider: 'unset',
  needs_consent: true,
  connected: true,
  claim_state: null,
  balance_micros: null,
  balance_line: null,
  cost_sentence: null,
  claim_url: null,
  add_credit_url: null,
  top_up_url: null,
  needs_credit: false,
  last_charge_micros: null,
  last_request_id: null,
  starter_remaining_micros: null,
  week_used_micros: null,
  week_budget_micros: null,
  week_resets_at: null,
  own_key_stored: false,
  local_model: null,
}

const FRESH_LINE = '$0.00 · link this computer for $0.05 of free use'

const STATUS_ANONYMOUS = {
  ...STATUS_UNSET,
  provider: 'publik',
  needs_consent: false,
  claim_state: 'anonymous',
  balance_micros: 0,
  balance_line: FRESH_LINE,
  cost_sentence: 'A typical use costs about $0.01 on publik-balanced and $0.001 on publik-fast.',
  claim_url: 'https://publikhq.com/claim/5f0c8a52',
  add_credit_url: 'https://publikhq.com/wallet/add-credit',
  top_up_url: 'https://publikhq.com/claim/5f0c8a52?from=wallet',
}

// ---- the fake bridge ------------------------------------------------------------------------------

function reply(command: string, answer: (args: Args) => unknown) {
  fake.answers.set(command, answer)
}

function callsOf(command: string): Args[] {
  return fake.calls.filter((call) => call.command === command).map((call) => call.args)
}

function answerDefaults() {
  fake.answers.clear()
  fake.calls.length = 0
  reply('app_start', () => ({ onboarded: true, active_course: 'es', notice: null }))
  reply('settings_get', () => SETTINGS)
  reply('settings_set', (args) => ({ ...SETTINGS, ...(args.patch as Args) }))
  reply('course_list', () => [
    { lang: 'es', name: 'Spanish', started: false, active: false, placement_done: false, cefr_estimate: null, skills: 8 },
  ])
  reply('path_get', () => PATH)
  reply('lesson_start', () => LESSON)
  reply('review_start', () => NOTHING_DUE)
  reply('exercise_submit', () => ({ verdict: 'correct', tags: [], message: null, corrected: null, words: [], rating: 3 }))
  reply('stats_get', () => FULL_STATS)
  reply('file_pick', () => null)
  reply('app_info', () => ({ version: '9.9.9', data_dir: 'parley-data-folder' }))
}

// ---- where the learner is -------------------------------------------------------------------------

/** The path's level-1 heading is the course's language. */
function onPath() {
  expect(screen.getByRole('heading', { level: 1, name: 'Spanish' })).toBeInTheDocument()
  expect(screen.queryByRole('heading', { level: 1, name: 'Stats' })).toBeNull()
}

function onScreen(title: 'Stats' | 'Settings' | 'Reviews') {
  expect(screen.getByRole('heading', { level: 1, name: title })).toBeInTheDocument()
  expect(screen.queryByRole('heading', { level: 1, name: 'Spanish' })).toBeNull()
}

async function openApp() {
  render(<App />)
  await screen.findByRole('heading', { level: 1, name: 'Spanish' })
  onPath()
}

/** Opens a screen from the path by the path's own button. */
async function openFromPath(which: 'Stats' | 'Settings' | 'Reviews') {
  const button = which === 'Reviews' ? 'Review (3)' : which
  fireEvent.click(await screen.findByRole('button', { name: button }))
  await screen.findByRole('heading', { level: 1, name: which })
  onScreen(which)
}

// ---- the keys and mouse buttons -------------------------------------------------------------------

/**
 * A key press where the learner's focus is; true when the app prevented the key's default action
 * (`fireEvent` returns what `dispatchEvent` returns: false once a listener called preventDefault).
 */
function press(key: string, init: { altKey?: boolean } = {}): boolean {
  return !fireEvent.keyDown(document.activeElement ?? document.body, { key, ...init })
}

const BACK_BUTTON = 3
const FORWARD_BUTTON = 4

/**
 * One press of a mouse's side button as the web view delivers it: mousedown, mouseup, then
 * auxclick, each with `button` 3 (back) or 4 (forward). Returns, for each of the three events,
 * whether its default action (the web view's own history navigation) was prevented.
 */
function mouseButton(button: number): boolean[] {
  const target = document.body
  const down = fireEvent.mouseDown(target, { button })
  const up = fireEvent.mouseUp(target, { button })
  const aux = fireEvent(target, new MouseEvent('auxclick', { button, bubbles: true, cancelable: true }))
  return [down, up, aux].map((dispatched) => !dispatched)
}

type Way = 'mouse' | 'keys'

/** Back by the mouse's back button or Alt+Left; asserts the web view's navigation was cancelled. */
function back(way: Way) {
  if (way === 'mouse') expect(mouseButton(BACK_BUTTON), 'mousedown, mouseup and auxclick prevented').toEqual([true, true, true])
  else expect(press('ArrowLeft', { altKey: true }), 'Alt+Left prevented').toBe(true)
}

/** Forward by the mouse's forward button or Alt+Right; the web view's navigation is cancelled. */
function forward(way: Way) {
  if (way === 'mouse') expect(mouseButton(FORWARD_BUTTON), 'mousedown, mouseup and auxclick prevented').toEqual([true, true, true])
  else expect(press('ArrowRight', { altKey: true }), 'Alt+Right prevented').toBe(true)
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  cleanup()
})

// ---- Back to my path at the top and the foot ---------------------------------------------------------

describe('Back to my path on a long screen', () => {
  it('Stats on a full course shows Back to my path before the skills list and after it, and both return to the path', async () => {
    await openApp()
    await openFromPath('Stats')
    const skillsHeading = await screen.findByRole('heading', { level: 2, name: 'Skills' })
    const skills = skillsHeading.closest('section') as HTMLElement
    expect(within(skills).getAllByRole('listitem')).toHaveLength(FULL_COURSE_SKILLS)

    const backs = screen.getAllByRole('button', { name: 'Back to my path' })
    expect(backs).toHaveLength(2)
    const [top, bottom] = backs
    expect(top.compareDocumentPosition(skills) & Node.DOCUMENT_POSITION_FOLLOWING, 'the skills list follows the top one').toBeTruthy()
    expect(skills.compareDocumentPosition(bottom) & Node.DOCUMENT_POSITION_FOLLOWING, 'the bottom one follows the skills list').toBeTruthy()
    // The top one comes before everything the screen lists: the 30-day calendar too.
    const calendar = screen.getByRole('heading', { level: 2, name: 'Last 30 days' })
    expect(top.compareDocumentPosition(calendar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()

    fireEvent.click(top)
    onPath()

    await openFromPath('Stats')
    await screen.findByRole('heading', { level: 2, name: 'Skills' })
    const again = screen.getAllByRole('button', { name: 'Back to my path' })
    fireEvent.click(again[again.length - 1])
    onPath()
  })

  it.each(['Settings', 'Reviews'] as const)('%s has Back to my path at its top and at its foot', async (which) => {
    await openApp()
    await openFromPath(which)
    const backs = screen.getAllByRole('button', { name: 'Back to my path' })
    expect(backs).toHaveLength(2)
    const heading = screen.getByRole('heading', { level: 1, name: which })
    // The top one comes straight after the heading, before anything else the screen shows.
    const main = heading.closest('main') as HTMLElement
    const controls = within(main).getAllByRole('button')
    expect(controls[0]).toBe(backs[0])
    expect(controls[controls.length - 1]).toBe(backs[1])
    fireEvent.click(backs[0])
    onPath()
  })
})

// ---- Esc leaves a screen the path opened ----------------------------------------------------------------

describe('Esc on a screen the path opened', () => {
  it.each(['Stats', 'Settings', 'Reviews'] as const)('Esc on %s returns to the path', async (which) => {
    await openApp()
    await openFromPath(which)
    expect(press('Escape'), 'Esc is taken').toBe(true)
    onPath()
  })

  it('Esc on the path itself does nothing', async () => {
    await openApp()
    await waitFor(() => expect(callsOf('path_get')).toHaveLength(1))
    expect(press('Escape')).toBe(false)
    onPath()
    expect(callsOf('path_get')).toHaveLength(1)
  })
})

// ---- the mouse buttons and Alt+arrows -------------------------------------------------------------

describe('Back and forward between screens', () => {
  it('the mouse back button on Stats returns to the path, and the forward button goes to Stats again', async () => {
    await openApp()
    await openFromPath('Stats')
    back('mouse')
    onPath()
    forward('mouse')
    onScreen('Stats')
    await screen.findByRole('heading', { level: 2, name: 'Skills' })
  })

  it('Alt+Left on Stats returns to the path, and Alt+Right goes to Stats again', async () => {
    await openApp()
    await openFromPath('Stats')
    back('keys')
    onPath()
    forward('keys')
    onScreen('Stats')
  })

  it('walks the screens visited in order, and opening a screen empties forward', async () => {
    await openApp()
    await openFromPath('Stats')
    // Back to my path opens the path again, as a new step: path, Stats, path, Settings.
    fireEvent.click(screen.getAllByRole('button', { name: 'Back to my path' })[0])
    onPath()
    await openFromPath('Settings')

    back('mouse')
    onPath()
    back('keys')
    onScreen('Stats')
    back('mouse')
    onPath()
    // The first screen: back does nothing, and the page is not left.
    back('keys')
    onPath()

    forward('mouse')
    onScreen('Stats')
    forward('keys')
    onPath()
    forward('mouse')
    onScreen('Settings')
    // The last screen: forward does nothing.
    forward('keys')
    onScreen('Settings')

    // Opening Reviews from the path drops Settings from forward.
    back('mouse')
    onPath()
    await openFromPath('Reviews')
    forward('mouse')
    onScreen('Reviews')
    back('keys')
    onPath()
    forward('keys')
    onScreen('Reviews')
    back('mouse')
    back('mouse')
    onScreen('Stats')
  })

  it('one press of a mouse button moves one screen, not one per event', async () => {
    await openApp()
    await openFromPath('Stats')
    fireEvent.click(screen.getAllByRole('button', { name: 'Back to my path' })[0])
    await openFromPath('Settings')
    // path, Stats, path, Settings: one press of back is the path, not Stats.
    back('mouse')
    onPath()
    forward('mouse')
    onScreen('Settings')
  })

  it.each(['mouse', 'keys'] as const)('back on the first screen does nothing and reloads nothing (%s)', async (way) => {
    await openApp()
    await waitFor(() => expect(callsOf('path_get')).toHaveLength(1))
    back(way)
    onPath()
    forward(way)
    onPath()
    expect(callsOf('path_get'), 'the path was not shown again').toHaveLength(1)
    expect(callsOf('app_start'), 'the app was not started again').toHaveLength(1)
  })

  it('Alt+Left in a text box is left to the text box', async () => {
    await openApp()
    await openFromPath('Settings')
    fireEvent.click(screen.getByRole('button', { name: 'Data' }))
    const box = screen.getByRole('textbox', { name: 'Type delete to confirm.' })
    box.focus()
    expect(press('ArrowLeft', { altKey: true }), 'the text box keeps Alt+Left').toBe(false)
    onScreen('Settings')
  })
})

// ---- a lesson and onboarding --------------------------------------------------------------------------

describe('Back, forward and Esc in a lesson and in onboarding', () => {
  it('Esc in a lesson still asks to quit; back and forward do nothing there, and the lesson empties forward', async () => {
    await openApp()
    await openFromPath('Stats')
    back('mouse')
    onPath()
    // Forward would lead to Stats; starting a lesson empties it.
    fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
    await screen.findByText('Exercise 1 of 2')

    back('mouse')
    forward('mouse')
    back('keys')
    forward('keys')
    expect(screen.getByText('Exercise 1 of 2')).toBeInTheDocument()
    expect(screen.queryByRole('dialog')).toBeNull()

    expect(press('Escape')).toBe(true)
    const dialog = await screen.findByRole('dialog', { name: 'Quit this lesson?' })
    // Back with the quit dialog open leaves nothing either.
    back('mouse')
    expect(screen.getByRole('dialog', { name: 'Quit this lesson?' })).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Keep going' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(screen.getByText('Exercise 1 of 2')).toBeInTheDocument()

    press('Escape')
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Quit' }))
    onPath()
    forward('mouse')
    onPath()
    back('mouse')
    onPath()
  })

  it('back, forward and Esc do nothing in onboarding', async () => {
    reply('app_start', () => ({ onboarded: false, active_course: null, notice: null }))
    render(<App />)
    await screen.findByRole('heading', { level: 1, name: 'Welcome to Parley' })
    back('mouse')
    back('keys')
    press('Escape')
    expect(screen.getByRole('heading', { level: 1, name: 'Welcome to Parley' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await screen.findByRole('heading', { level: 1, name: 'Choose a language' })
    back('mouse')
    back('keys')
    press('Escape')
    expect(screen.getByRole('heading', { level: 1, name: 'Choose a language' })).toBeInTheDocument()
    forward('mouse')
    expect(screen.getByRole('heading', { level: 1, name: 'Choose a language' })).toBeInTheDocument()
  })
})

// ---- dialogs keep their own Esc ----------------------------------------------------------------------

describe('Esc with a dialog open', () => {
  async function openRestoreDialog() {
    await openApp()
    await openFromPath('Settings')
    fireEvent.click(screen.getByRole('button', { name: 'Data' }))
    fireEvent.click(screen.getByRole('button', { name: 'Restore a backup' }))
    return screen.findByRole('dialog')
  }

  it('Esc closes the restore dialog and stays on Settings > Data; a second Esc returns to the path', async () => {
    await openRestoreDialog()
    expect(press('Escape')).toBe(true)
    expect(screen.queryByRole('dialog')).toBeNull()
    onScreen('Settings')
    expect(screen.getByRole('button', { name: 'Export a backup' })).toBeInTheDocument()
    expect(callsOf('file_pick')).toHaveLength(0)
    expect(callsOf('backup_import')).toHaveLength(0)

    press('Escape')
    onPath()
  })

  it('back and forward leave nothing while a dialog is open', async () => {
    await openRestoreDialog()
    back('mouse')
    back('keys')
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    onScreen('Settings')
  })

  it('Esc in the AI setup dialog closes it as Not now does, and on its first-run card closes it too', async () => {
    reply('ai_status', () => STATUS_UNSET)
    reply('ai_consent_decline', () => STATUS_UNSET)
    reply('ai_consent_accept', () => STATUS_ANONYMOUS)
    await openApp()
    fireEvent.click(await screen.findByRole('button', { name: 'Set up AI' }))
    await screen.findByRole('dialog', { name: 'Set up AI features' })
    expect(press('Escape')).toBe(true)
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(callsOf('ai_consent_decline')).toHaveLength(1)
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
    onPath()

    fireEvent.click(await screen.findByRole('button', { name: 'Set up AI' }))
    const dialog = await screen.findByRole('dialog', { name: 'Set up AI features' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Continue with publik API' }))
    await within(dialog).findByText(FRESH_LINE)
    expect(press('Escape')).toBe(true)
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    onPath()
  })
})

// ---- a long screen scrolls --------------------------------------------------------------------------

/** The declarations of every rule whose selector list names `selector`. */
function declarationsFor(css: string, selector: string): Map<string, string>[] {
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((match) => match[1].split(',').some((part) => part.trim() === selector))
    .map((match) => {
      const declarations = new Map<string, string>()
      for (const declaration of match[2].split(';')) {
        const colon = declaration.indexOf(':')
        if (colon > 0) declarations.set(declaration.slice(0, colon).trim(), declaration.slice(colon + 1).trim())
      }
      return declarations
    })
}

describe('A long screen scrolls to its foot', () => {
  it('nothing in the stylesheet clips or fixes the height of the page', () => {
    const css = STYLESHEET.replace(/\/\*[\s\S]*?\*\//g, '')
    for (const selector of [':root', 'html', 'body', '#root', 'main', '.stats']) {
      for (const declarations of declarationsFor(css, selector)) {
        for (const property of ['overflow', 'overflow-y', 'overflow-block']) {
          expect(declarations.get(property) ?? 'visible', `${selector} ${property}`).not.toMatch(/hidden|clip/)
        }
        expect(declarations.has('height'), `${selector} sets no height`).toBe(false)
        expect(declarations.has('max-height'), `${selector} sets no max-height`).toBe(false)
        expect(declarations.get('position') ?? 'static', `${selector} is not fixed`).not.toBe('fixed')
      }
    }
  })

  it('the scrolling keys keep their default on Stats', async () => {
    await openApp()
    await openFromPath('Stats')
    await screen.findByRole('heading', { level: 2, name: 'Skills' })
    for (const key of ['PageDown', 'PageUp', 'ArrowDown', 'ArrowUp', 'ArrowLeft', 'ArrowRight', 'End', 'Home', ' ']) {
      expect(press(key), `${key === ' ' ? 'Space' : key} is left to the page`).toBe(false)
    }
    onScreen('Stats')
  })
})
