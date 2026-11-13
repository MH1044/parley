// Module 04 UI acceptance tests (docs/modules/04-app-shell-and-offline-course.md): the app is
// rendered whole, the Tauri bridge is faked with the spec's result shapes, and every element is
// found by the visible text and roles of the spec's UI contract.

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

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
    autoplay: true,
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

const COURSES = [
  { lang: 'es', name: 'Spanish', started: false, active: false, placement_done: false, cefr_estimate: null, skills: 8 },
  { lang: 'fr', name: 'French', started: false, active: false, placement_done: false, cefr_estimate: null, skills: 8 },
  { lang: 'de', name: 'German', started: false, active: false, placement_done: false, cefr_estimate: null, skills: 8 },
]

const PACK_AVAILABLE = { lang: 'es', state: 'available', version: '2026-10-01', bytes: 8_200_000, size: '8.2 MB' }

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
        {
          id: 'es.a1.u01.s02',
          title: 'Names',
          can_do: 'I can say my name and ask someone their name.',
          cefr: 'A1',
          state: 'locked',
          mastery: 0,
          tested_out: false,
        },
      ],
    },
    {
      id: 'es.a1.u02',
      n: 2,
      title: 'Day to day',
      cefr: 'A1',
      skills: [
        {
          id: 'es.a1.u02.s01',
          title: 'Family',
          can_do: 'I can talk about the people in my family.',
          cefr: 'A1',
          state: 'locked',
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

const OPEN_SKILL = {
  id: 'es.a1.u01.s01',
  title: 'Greetings',
  can_do: 'I can say hello and goodbye and ask how someone is.',
  cefr: 'A1',
  grammar_note: 'Questions and exclamations open with an upside-down mark.',
  unit_title: 'Hello',
  state: 'available',
  mastery: 0,
  words: [
    { target: 'hola', en: 'hello' },
    { target: 'adiós', en: 'goodbye' },
  ],
  opened_by: null,
}

const LOCKED_SKILL = {
  ...OPEN_SKILL,
  id: 'es.a1.u01.s02',
  title: 'Names',
  can_do: 'I can say my name and ask someone their name.',
  state: 'locked',
  words: [{ target: 'el nombre', en: 'name' }],
  opened_by: 'Greetings',
}

const SESSION = 's-00000000000000a1'
const PLACEMENT = 'p-00000000000000b1'

function question(n: number, skillId: string, itemId: string, prompt: string, accepted: string[]) {
  return {
    session_id: PLACEMENT,
    n,
    skill_id: skillId,
    exercise: {
      id: `p${n}`,
      item_id: itemId,
      facet: 'receptive',
      origin: 'template',
      type: 'translate_to_en',
      prompt_target: prompt,
      accepted,
    },
  }
}

const QUESTION_1 = question(1, 'es.a1.u01.s03', 'es.a1.u01.s03#l:ser', 'ser', ['to be'])
const QUESTION_2 = question(2, 'es.a1.u01.s02', 'es.a1.u01.s02#l:llamarse', 'llamarse', ['to be called'])
const AROUND_A1 = {
  estimate: 3,
  label: 'around A1',
  tested_out: 3,
  start_skill_id: 'es.a1.u01.s04',
  start_unit: 1,
  start_unit_title: 'Hello',
}

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
  id: 'e2',
  item_id: 'es.a1.u01.s01#l:adiós',
  facet: 'receptive',
  origin: 'template',
  type: 'pick_meaning',
  prompt_target: 'adiós',
  options: ['night', 'thank you', 'goodbye', 'hello'],
  answer_index: 2,
}

const FILL = {
  id: 'e1',
  item_id: 'es.a1.u01.s01#l:hola',
  facet: 'productive',
  origin: 'template',
  type: 'fill_gap',
  sentence: '___, soy Ana.',
  answer: 'Hola',
  full_sentence: 'Hola, soy Ana.',
  hint_en: 'hello',
}

const TRANSLATE_EN = {
  id: 'e1',
  item_id: 'es.a1.u01.s01#e:2',
  facet: 'receptive',
  origin: 'template',
  type: 'translate_to_en',
  prompt_target: 'Buenos días.',
  accepted: ['Good morning.'],
}

const WORD_BANK = {
  id: 'e1',
  item_id: 'es.a1.u02.s02#e:2',
  facet: 'productive',
  origin: 'template',
  type: 'translate_to_target',
  prompt_en: 'I want a beer.',
  target: 'Quiero una cerveza.',
  accepted: ['Quiero una cerveza.'],
  tiles: ['una', 'leche', 'quiero', 'cerveza', 'el'],
}

const MATCH = {
  id: 'e1',
  item_id: 'es.a1.u01.s01#l:hola',
  facet: 'receptive',
  origin: 'template',
  type: 'match_pairs',
  pairs: [
    ['hola', 'hello'],
    ['adiós', 'goodbye'],
    ['gracias', 'thank you'],
    ['noche', 'night'],
  ],
}

function lesson(exercises: object[], attributions: { exercise_id: string; author: string }[] = []) {
  return {
    session_id: SESSION,
    kind: 'lesson',
    skill_id: 'es.a1.u01.s01',
    origin: 'template',
    exercises,
    attributions,
    notice: null,
  }
}

function grade(verdict: string, extra: Partial<{ message: string | null; corrected: string | null; rating: number | null }> = {}) {
  return { verdict, tags: [], message: null, corrected: null, words: [], rating: 3, ...extra }
}

const SUMMARY = {
  xp: 17,
  correct: 2,
  total: 2,
  streak: 5,
  freezes: 1,
  record_week: true,
  message: 'New weekly record!',
  daily_xp: 29,
  goal_xp: 20,
}

const EMPTY_STATS = {
  lang: 'es',
  empty: true,
  streak: 0,
  freezes: 0,
  days: [],
  week_xp: 0,
  record_week_xp: 0,
  words_known: 0,
  vocab_items: 76,
  skills: [],
  cefr_estimate: null,
  recall_rate: null,
  reviews_30: 0,
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

// ---- the fake bridge ------------------------------------------------------------------------------

function reply(command: string, answer: (args: Args) => unknown) {
  fake.answers.set(command, answer)
}

function callsOf(command: string): Args[] {
  return fake.calls.filter((call) => call.command === command).map((call) => call.args)
}

function appError(code: string, message: string, action: Record<string, unknown> | null = null) {
  return { code, message, action }
}

function failWith(error: ReturnType<typeof appError>) {
  return () => Promise.reject(error)
}

function answerDefaults() {
  fake.answers.clear()
  fake.calls.length = 0
  let settings: Record<string, unknown> = { ...SETTINGS }
  let placementAnswers = 0
  reply('app_start', () => ({ onboarded: false, active_course: null, notice: null }))
  reply('app_info', () => ({ version: '9.9.9', data_dir: 'parley-data-folder' }))
  reply('file_pick', () => null)
  reply('data_delete_all', () => null)
  reply('cache_clear', () => ({ files_removed: 0 }))
  reply('settings_get', () => settings)
  reply('settings_set', (args) => {
    settings = { ...settings, ...(args.patch as Record<string, unknown>) }
    return settings
  })
  reply('course_list', () => COURSES)
  reply('course_start', (args) => ({ ...COURSES[0], lang: args.lang, started: true, active: true }))
  reply('pack_status', () => PACK_AVAILABLE)
  reply('pack_download', () => ({ ...PACK_AVAILABLE, state: 'installed' }))
  reply('path_get', () => PATH)
  reply('skill_get', (args) => (args.skill_id === LOCKED_SKILL.id ? LOCKED_SKILL : OPEN_SKILL))
  reply('placement_start', () => QUESTION_1)
  reply('placement_answer', () => {
    placementAnswers += 1
    return placementAnswers === 1 ? { next: QUESTION_2, result: null } : { next: null, result: AROUND_A1 }
  })
  reply('lesson_start', () => lesson([PICK_1, PICK_2]))
  reply('review_start', () => NOTHING_DUE)
  reply('exercise_submit', () => grade('correct'))
  reply('exercise_i_was_right', () => grade('correct'))
  reply('lesson_finish', () => SUMMARY)
  reply('stats_get', () => EMPTY_STATS)
  reply('backup_export', () => ({ bytes: 1234, courses: 1, cards: 10, reviews: 20 }))
  reply('backup_import', () => ({ courses: 1, items: 0, cards: 10, reviews: 20, skipped_cards: 0 }))
}

function onboarded() {
  reply('app_start', () => ({ onboarded: true, active_course: 'es', notice: null }))
}

// ---- finding things -------------------------------------------------------------------------------

const squash = (text: string | null | undefined) => (text ?? '').replace(/\s+/g, ' ').trim()

/** The innermost elements whose text contains `text`, even when it is split over child elements. */
function textOf(text: string) {
  return (_content: string, element: Element | null): boolean =>
    element !== null &&
    squash(element.textContent).includes(text) &&
    !Array.from(element.children).some((child) => squash(child.textContent).includes(text))
}

async function expectText(text: string, container: HTMLElement = document.body) {
  await waitFor(() => expect(within(container).queryAllByText(textOf(text)).length).toBeGreaterThan(0))
}

function expectNoDialog() {
  expect(screen.queryByRole('dialog')).toBeNull()
}

/** A key press where the learner's focus is. */
function press(key: string) {
  const code = /^\d$/.test(key) ? `Digit${key}` : key
  fireEvent.keyDown(document.activeElement ?? document.body, { key, code })
}

async function heading(name: string, level?: number) {
  return screen.findByRole('heading', level === undefined ? { name } : { name, level })
}

async function walkTo(step: 'Course pack' | 'Daily goal' | 'Where to start') {
  render(<App />)
  await heading('Welcome to Parley', 1)
  fireEvent.click(screen.getByRole('button', { name: 'Start' }))
  await heading('Choose a language', 1)
  fireEvent.click(screen.getByRole('button', { name: 'Spanish' }))
  await heading('Course pack', 1)
  if (step === 'Course pack') return
  fireEvent.click(await screen.findByRole('button', { name: 'Skip for now' }))
  await heading('Daily goal', 1)
  if (step === 'Daily goal') return
  fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
  await heading('Where to start', 1)
}

async function startLesson() {
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
}

async function openSettings(tab: 'Learning' | 'Data' | 'About') {
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
  await heading('Settings')
  for (const name of ['Learning', 'Data', 'About']) {
    expect(screen.getByRole('button', { name })).toBeInTheDocument()
  }
  fireEvent.click(screen.getByRole('button', { name: tab }))
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  cleanup()
})

// ---- acceptance test 1: onboarding --------------------------------------------------------------

describe('Onboarding (acceptance test 1, invariant I12)', () => {
  it('shows Welcome, Choose a language, Course pack, Daily goal and Where to start, in order, with no dialog', async () => {
    render(<App />)
    const seen: string[] = []
    let previous: string | null = null
    const step = async (name: string) => {
      await heading(name, 1)
      if (previous !== null) {
        expect(screen.queryByRole('heading', { level: 1, name: previous })).toBeNull()
      }
      expectNoDialog()
      seen.push(name)
      previous = name
    }

    await step('Welcome to Parley')
    await expectText('Learn by speaking. No hearts, no ads, nothing to buy.')
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))

    await step('Choose a language')
    await expectText('Parley teaches from English')
    for (const name of ['Spanish', 'French', 'German']) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument()
    }
    fireEvent.click(screen.getByRole('button', { name: 'Spanish' }))
    await waitFor(() => expect(callsOf('course_start')).toEqual([{ lang: 'es' }]))

    await step('Course pack')
    await expectText('Download the Spanish course pack (8.2 MB)?')
    expect(screen.getByRole('button', { name: 'Download' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Skip for now' }))
    expect(callsOf('pack_download')).toHaveLength(0)

    await step('Daily goal')
    for (const name of ['10 XP', '30 XP', '50 XP']) {
      expect(screen.getByRole('radio', { name })).not.toBeChecked()
    }
    expect(screen.getByRole('radio', { name: '20 XP' })).toBeChecked()
    fireEvent.click(screen.getByRole('radio', { name: '30 XP' }))
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(callsOf('settings_set')).toContainEqual({ patch: { daily_goal_xp: 30 } }))

    await step('Where to start')
    expect(screen.getByRole('button', { name: 'Take the placement test (about 5 minutes)' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Start from the beginning' })).toBeInTheDocument()

    expect(seen).toEqual(['Welcome to Parley', 'Choose a language', 'Course pack', 'Daily goal', 'Where to start'])
  })

  it.each([
    ['not_published', 'No course pack is published for Spanish yet. Parley works without it, with fewer example sentences.'],
    ['installed', 'The Spanish course pack is installed.'],
  ])('the Course pack step when the pack is %s', async (state, text) => {
    reply('pack_status', () => ({
      lang: 'es',
      state,
      version: state === 'installed' ? '2026-10-01' : null,
      bytes: state === 'installed' ? 8_200_000 : null,
      size: state === 'installed' ? '8.2 MB' : null,
    }))
    await walkTo('Course pack')
    await expectText(text)
    expect(screen.queryByRole('button', { name: 'Download' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await heading('Daily goal', 1)
    expectNoDialog()
  })

  it('a failed download shows its message and Continue', async () => {
    const offline = "You're offline. Parley works without the pack, with fewer example sentences."
    reply('pack_download', failWith(appError('offline', offline)))
    await walkTo('Course pack')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))
    await waitFor(() => expect(callsOf('pack_download')).toEqual([{ lang: 'es' }]))
    await expectText(offline)
    expectNoDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await heading('Daily goal', 1)
  })

  it('a download that finds no published pack says so and goes on without it', async () => {
    // pack_download's answer when the pack URL answers 404 or 410.
    reply('pack_download', () => ({ lang: 'es', state: 'not_published', version: null, bytes: null, size: null }))
    await walkTo('Course pack')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))
    await waitFor(() => expect(callsOf('pack_download')).toEqual([{ lang: 'es' }]))
    await expectText(
      'No course pack is published for Spanish yet. Parley works without it, with fewer example sentences.',
    )
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByText(textOf('Try again later'))).toBeNull()
    expect(screen.queryByRole('button', { name: 'Download' })).toBeNull()
    expectNoDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await heading('Daily goal', 1)
  })

  it('the placement test asks, accepts I don\'t know, and says where the learner starts', async () => {
    await walkTo('Where to start')
    fireEvent.click(screen.getByRole('button', { name: 'Take the placement test (about 5 minutes)' }))
    await waitFor(() => expect(callsOf('placement_start')).toEqual([{ lang: 'es' }]))
    await heading('Placement test')
    await expectText('Question 1')
    expect(screen.getByRole('textbox', { name: 'Your answer' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Next' })).toBeInTheDocument()
    expectNoDialog()

    fireEvent.click(screen.getByRole('button', { name: "I don't know" }))
    await waitFor(() =>
      expect(callsOf('placement_answer')).toEqual([{ session_id: PLACEMENT, answer: { text: '' } }]),
    )
    await expectText('Question 2')
    fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: 'to be called' } })
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() =>
      expect(callsOf('placement_answer')[1]).toEqual({ session_id: PLACEMENT, answer: { text: 'to be called' } }),
    )

    await expectText("You're around A1. You'll start at Unit 1: Hello.")
    expectNoDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Go to my path' }))
    await waitFor(() => expect(callsOf('path_get')).toContainEqual({ lang: 'es' }))
  })

  it('a Pre-A1 result names only where the learner starts', async () => {
    reply('placement_answer', () => ({
      next: null,
      result: { ...AROUND_A1, estimate: 0, label: 'Pre-A1', tested_out: 0, start_skill_id: 'es.a1.u01.s01' },
    }))
    await walkTo('Where to start')
    fireEvent.click(screen.getByRole('button', { name: 'Take the placement test (about 5 minutes)' }))
    await heading('Placement test')
    fireEvent.click(screen.getByRole('button', { name: "I don't know" }))
    await expectText("You'll start at Unit 1: Hello.")
    expect(screen.queryAllByText(textOf("You're Pre-A1"))).toHaveLength(0)
  })
})

// ---- the path, reviews and stats (UI contract) -------------------------------------------------

describe('Path, reviews and stats', () => {
  beforeEach(() => {
    onboarded()
  })

  it('shows the streak, the day\'s XP, the units and their skills', async () => {
    render(<App />)
    await expectText('4 day streak')
    await expectText('12 of 20 XP today')
    for (const name of ['Continue', 'Review (3)', 'Stats', 'Settings']) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument()
    }
    expect(screen.getByRole('heading', { level: 2, name: 'Unit 1: Hello' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 2, name: 'Unit 2: Day to day' })).toBeInTheDocument()
    for (const name of ['Greetings', 'Names', 'Family']) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument()
    }
    expect(callsOf('path_get')).toContainEqual({ lang: 'es' })
  })

  it('the launch notice shows as a banner', async () => {
    const notice = 'Your settings file could not be read, so Parley started with default settings.'
    reply('app_start', () => ({ onboarded: true, active_course: 'es', notice }))
    render(<App />)
    await expectText(notice)
  })

  it('a skill opens its detail; a locked one names the skill that opens it', async () => {
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Names' }))
    await waitFor(() => expect(callsOf('skill_get')).toContainEqual({ skill_id: 'es.a1.u01.s02' }))
    await expectText('Finish Greetings to open this skill.')
  })

  it('Start lesson in an open skill starts a lesson of that skill', async () => {
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Greetings' }))
    await expectText('I can say hello and goodbye and ask how someone is.')
    await expectText('hola')
    fireEvent.click(await screen.findByRole('button', { name: 'Start lesson' }))
    await waitFor(() => expect(callsOf('lesson_start')).toContainEqual({ skill_id: 'es.a1.u01.s01', mode: 'learn' }))
  })

  it('Continue starts a lesson of the next skill', async () => {
    await startLesson()
    await waitFor(() => expect(callsOf('lesson_start')).toEqual([{ skill_id: 'es.a1.u01.s01', mode: 'learn' }]))
    await expectText('Exercise 1 of 2')
  })

  it('a review with nothing due shows the notice and Back to my path', async () => {
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Review (3)' }))
    await waitFor(() => expect(callsOf('review_start')).toEqual([{ lang: 'es', limit: 20 }]))
    await expectText('Nothing to review. Next review: tomorrow at 09:00.')
    fireEvent.click(screen.getByRole('button', { name: 'Back to my path' }))
    await expectText('4 day streak')
  })

  it('Stats with nothing finished asks for a lesson', async () => {
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Stats' }))
    await heading('Stats')
    await waitFor(() => expect(callsOf('stats_get')).toContainEqual({ lang: 'es' }))
    await expectText('Finish a lesson to see your stats.')
  })
})

// ---- acceptance test 12: the keyboard ---------------------------------------------------------------

describe('Keyboard in the lesson player (acceptance test 12)', () => {
  beforeEach(() => {
    onboarded()
  })

  it('key 2 checks option 2, Enter submits it, Enter on the result continues, Esc asks to quit', async () => {
    await startLesson()
    await expectText('Exercise 1 of 2')
    await expectText('What does hola mean?')
    const group = screen.getByRole('radiogroup')
    expect(within(group).getAllByRole('radio')).toHaveLength(4)

    press('2')
    await waitFor(() => expect(within(group).getByRole('radio', { name: 'hello' })).toBeChecked())
    expect(callsOf('exercise_submit')).toHaveLength(0)

    press('Enter')
    await waitFor(() => expect(callsOf('exercise_submit')).toHaveLength(1))
    expect(callsOf('exercise_submit')[0]).toMatchObject({
      session_id: SESSION,
      exercise_id: 'e1',
      answer: { choice: 1 },
      used_hint: false,
    })
    const result = await screen.findByRole('region', { name: 'Result' })
    await expectText('Correct', result)

    press('Enter')
    await expectText('Exercise 2 of 2')
    await expectText('What does adiós mean?')

    press('Escape')
    const dialog = await screen.findByRole('dialog')
    await expectText('Quit this lesson?', dialog)
    expect(within(dialog).getByRole('button', { name: 'Quit' })).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Keep going' }))
    await waitFor(() => expectNoDialog())
    await expectText('Exercise 2 of 2')
  })
})

// ---- acceptance test 13 (UI half) and the result sheet -----------------------------------------

describe('Result sheet (acceptance test 13)', () => {
  beforeEach(() => {
    onboarded()
  })

  async function answerTheGap() {
    await startLesson()
    await expectText('Fill the gap')
    await expectText('hello')
    fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: 'Hola' } })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await waitFor(() =>
      expect(callsOf('exercise_submit')[0]).toMatchObject({
        session_id: SESSION,
        exercise_id: 'e1',
        answer: { text: 'Hola' },
      }),
    )
    return screen.findByRole('region', { name: 'Result' })
  }

  it('credits the author of a pack sentence', async () => {
    reply('lesson_start', () => lesson([FILL], [{ exercise_id: 'e1', author: 'fixture_a' }]))
    const result = await answerTheGap()
    await expectText('Correct', result)
    await expectText('Sentence by fixture_a on Tatoeba', result)
    expect(within(result).queryByRole('button', { name: 'I was right' })).toBeNull()
    expect(within(result).getByRole('button', { name: 'Continue' })).toBeInTheDocument()
  })

  it('says Sentence from Tatoeba when the sentence has no author', async () => {
    reply('lesson_start', () => lesson([FILL], [{ exercise_id: 'e1', author: '' }]))
    const result = await answerTheGap()
    await expectText('Sentence from Tatoeba', result)
    expect(within(result).queryAllByText(textOf('Sentence by'))).toHaveLength(0)
  })

  it('credits nobody for a syllabus sentence, and a wrong answer shows the right one', async () => {
    reply('lesson_start', () => lesson([FILL]))
    reply('exercise_submit', () => grade('wrong', { corrected: 'Hola', rating: 1 }))
    const result = await answerTheGap()
    await expectText('Wrong', result)
    await expectText('Correct answer: Hola', result)
    expect(within(result).queryAllByText(textOf('Tatoeba'))).toHaveLength(0)
    expect(within(result).queryByRole('button', { name: 'I was right' })).toBeNull()
  })

  it('shows Almost and its message', async () => {
    reply('lesson_start', () => lesson([FILL]))
    reply('exercise_submit', () =>
      grade('almost', { message: 'Almost: mind the accent: días', corrected: 'Buenos días', rating: 2 }),
    )
    const result = await answerTheGap()
    await expectText('Almost', result)
    await expectText('Almost: mind the accent: días', result)
    await expectText('Correct answer: Buenos días', result)
  })

  it('offers I was right for an unchecked answer only', async () => {
    reply('lesson_start', () => lesson([TRANSLATE_EN]))
    reply('exercise_submit', () => grade('unchecked', { rating: null }))
    await startLesson()
    await expectText('Translate into English')
    fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: 'Morning, everyone.' } })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const result = await screen.findByRole('region', { name: 'Result' })
    await expectText('Not checked', result)
    fireEvent.click(within(result).getByRole('button', { name: 'I was right' }))
    await waitFor(() =>
      expect(callsOf('exercise_i_was_right')).toEqual([{ session_id: SESSION, exercise_id: 'e1' }]),
    )
  })

  it('builds the answer from word-bank tiles', async () => {
    reply('settings_get', () => ({ ...SETTINGS, answer_input: 'word_bank' }))
    reply('lesson_start', () => lesson([WORD_BANK]))
    await startLesson()
    await expectText('Translate into Spanish')
    for (const tile of ['quiero', 'una', 'cerveza']) {
      fireEvent.click(await screen.findByRole('button', { name: tile }))
    }
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await waitFor(() =>
      expect(callsOf('exercise_submit')[0]).toMatchObject({ exercise_id: 'e1', answer: { text: 'quiero una cerveza' } }),
    )
  })

  it('matches pairs, a target then its English side', async () => {
    reply('lesson_start', () => lesson([MATCH]))
    await startLesson()
    for (const [target, english] of MATCH.pairs) {
      fireEvent.click(await screen.findByRole('button', { name: target }))
      fireEvent.click(await screen.findByRole('button', { name: english }))
    }
    const check = screen.queryByRole('button', { name: 'Check' })
    if (check) fireEvent.click(check)
    await waitFor(() => expect(callsOf('exercise_submit')).toHaveLength(1))
    const sent = callsOf('exercise_submit')[0].answer as { pairs: number[][] }
    const pairs = [...sent.pairs].map((pair) => pair.join(',')).sort()
    expect(pairs).toEqual(['0,0', '1,1', '2,2', '3,3'])
  })

  it('ends with the lesson summary', async () => {
    reply('lesson_start', () => lesson([PICK_1]))
    await startLesson()
    fireEvent.click(await screen.findByRole('radio', { name: 'hello' }))
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const result = await screen.findByRole('region', { name: 'Result' })
    fireEvent.click(within(result).getByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(callsOf('lesson_finish')).toEqual([{ session_id: SESSION }]))
    await heading('Lesson complete')
    await expectText('+17 XP')
    await expectText('2 of 2 correct')
    await expectText('5 day streak')
    await expectText('New weekly record!')
    expect(screen.getByRole('button', { name: 'Continue' })).toBeInTheDocument()
  })
})

// ---- acceptance tests 10 and 13: settings ----------------------------------------------------------

describe('Settings (acceptance tests 10 and 13)', () => {
  beforeEach(() => {
    onboarded()
  })

  it('Delete everything sends the typed word, starts again and shows Welcome to Parley', async () => {
    await openSettings('Data')
    fireEvent.change(screen.getByRole('textbox', { name: 'Type delete to confirm.' }), {
      target: { value: 'delete' },
    })
    const launches = callsOf('app_start').length
    reply('app_start', () => ({ onboarded: false, active_course: null, notice: null }))
    fireEvent.click(screen.getByRole('button', { name: 'Delete everything' }))
    await waitFor(() => expect(callsOf('data_delete_all')).toEqual([{ confirm_text: 'delete' }]))
    await waitFor(() => expect(callsOf('app_start').length).toBeGreaterThan(launches))
    await heading('Welcome to Parley', 1)
  })

  it('Restore a backup asks first, and Cancel imports nothing', async () => {
    reply('file_pick', () => 'backups/parley-backup.json')
    await openSettings('Data')
    fireEvent.click(screen.getByRole('button', { name: 'Restore a backup' }))
    const dialog = await screen.findByRole('dialog')
    await expectText(
      'Importing replaces all your progress on this computer. Export a backup first if you want to keep it.',
      dialog,
    )
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expectNoDialog())
    expect(callsOf('backup_import')).toHaveLength(0)
  })

  it('Replace my data imports the chosen file', async () => {
    reply('file_pick', () => 'backups/parley-backup.json')
    await openSettings('Data')
    fireEvent.click(screen.getByRole('button', { name: 'Restore a backup' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Replace my data' }))
    await waitFor(() => expect(callsOf('backup_import')).toEqual([{ src_path: 'backups/parley-backup.json' }]))
    expect(callsOf('file_pick')).toContainEqual({ kind: 'backup_import' })
  })

  it('Export a backup writes to the chosen file, and nothing when the picker is cancelled', async () => {
    await openSettings('Data')
    fireEvent.click(screen.getByRole('button', { name: 'Export a backup' }))
    await waitFor(() => expect(callsOf('file_pick')).toEqual([{ kind: 'backup_export' }]))
    expect(callsOf('backup_export')).toHaveLength(0)

    reply('file_pick', () => 'backups/parley-backup.json')
    fireEvent.click(screen.getByRole('button', { name: 'Export a backup' }))
    await waitFor(() => expect(callsOf('backup_export')).toEqual([{ dest_path: 'backups/parley-backup.json' }]))
  })

  it('About shows the version and data folder and credits Tatoeba and Wiktionary', async () => {
    await openSettings('About')
    await expectText('9.9.9')
    await expectText('parley-data-folder')
    await expectText('Example sentences come from Tatoeba (tatoeba.org) under CC BY 2.0 FR.')
    await expectText('Word forms come from Wiktionary (wiktionary.org) under CC BY-SA 4.0.')
  })
})

// ---- Check with no answer sends nothing -------------------------------------------------------

describe('Check with no answer', () => {
  beforeEach(() => {
    onboarded()
  })

  it.each([
    ['pick_meaning with no option chosen', [PICK_1], 'Choose an answer first.'],
    ['an empty fill_gap', [FILL], 'Type an answer first.'],
    ['an empty translation', [TRANSLATE_EN], 'Type an answer first.'],
  ])('%s says so and sends nothing', async (_what, exercises, message) => {
    reply('lesson_start', () => lesson(exercises as object[]))
    await startLesson()
    await expectText('Exercise 1 of 1')
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await expectText(message as string)
    expect(callsOf('exercise_submit')).toHaveLength(0)
    expect(screen.queryByRole('region', { name: 'Result' })).toBeNull()
  })

  it('match_pairs with one pair made asks for every pair and sends nothing', async () => {
    reply('lesson_start', () => lesson([MATCH]))
    await startLesson()
    const [target, english] = MATCH.pairs[0]
    fireEvent.click(await screen.findByRole('button', { name: target }))
    fireEvent.click(await screen.findByRole('button', { name: english }))
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await expectText('Match every pair first.')
    expect(callsOf('exercise_submit')).toHaveLength(0)
    expect(screen.queryByRole('region', { name: 'Result' })).toBeNull()
  })
})

// ---- a launch that fails for another reason --------------------------------------------------

describe('A failed launch', () => {
  it('shows the error with Try again, which starts the app again', async () => {
    reply('app_start', failWith(appError('invalid_input', 'Boom.')))
    render(<App />)
    await expectText('Something went wrong: Boom.')
    const launches = callsOf('app_start').length
    expect(launches).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(callsOf('app_start').length).toBeGreaterThan(launches))
  })
})

// ---- acceptance test 15 (UI half): the data-file error screen ----------------------------------

describe('Data-file error screen (acceptance test 15)', () => {
  it('shows the message, Restore a backup and Show details', async () => {
    reply(
      'app_start',
      failWith(appError('storage', 'Parley could not open its data file.', { details: 'file is not a database' })),
    )
    render(<App />)
    await expectText('Parley could not open its data file.')
    expect(screen.getByRole('button', { name: 'Restore a backup' })).toBeInTheDocument()
    for (const hidden of screen.queryAllByText(textOf('file is not a database'))) {
      expect(hidden).not.toBeVisible()
    }
    fireEvent.click(screen.getByRole('button', { name: 'Show details' }))
    await waitFor(() => {
      const shown = screen.queryAllByText(textOf('file is not a database'))
      expect(shown.length).toBeGreaterThan(0)
      shown.forEach((element) => expect(element).toBeVisible())
    })
  })
})
