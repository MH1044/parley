// Module 04 UI acceptance tests (docs/modules/04-app-shell-and-offline-course.md): the app is
// rendered whole, the Tauri bridge is faked with the spec's result shapes, and every element is
// found by the visible text and roles of the spec's UI contract.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
// The stylesheet as text (vite.config.ts lets this one import through as text).
import STYLESHEET from '../../index.css?raw'

type Args = Record<string, unknown>

type TauriEvent = { event: string; id: number; payload: unknown }

const fake = vi.hoisted(() => ({
  answers: new Map<string, (args: Record<string, unknown>) => unknown>(),
  calls: [] as { command: string; args: Record<string, unknown> }[],
  listeners: new Map<string, ((event: { event: string; id: number; payload: unknown }) => void)[]>(),
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

// `listen` keeps each handler until its unlisten function runs, so a test can deliver events.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (event: string, handler: (event: { event: string; id: number; payload: unknown }) => void) => {
    fake.listeners.set(event, [...(fake.listeners.get(event) ?? []), handler])
    return () => {
      fake.listeners.set(
        event,
        (fake.listeners.get(event) ?? []).filter((kept) => kept !== handler),
      )
    }
  }),
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
  // The meanings are shown as A thank you, B hello, C night, D goodbye.
  english_order: [2, 0, 3, 1],
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
  checked: 2,
  unchecked: 0,
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

/** Delivers `payload` on `event` to every handler listening for it. */
function emitEvent(event: string, payload: unknown) {
  const delivered: TauriEvent = { event, id: 1, payload }
  act(() => {
    for (const handler of fake.listeners.get(event) ?? []) {
      handler(delivered)
    }
  })
}

function answerDefaults() {
  fake.answers.clear()
  fake.calls.length = 0
  fake.listeners.clear()
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

  it('the bar fills as the pack arrives and says how far it is', async () => {
    const total = 174_747_648
    const progressAt = (done: number) => ({ lang: 'es', bytes_done: done, bytes_total: total })
    reply('pack_status', () => ({ lang: 'es', state: 'available', version: '2026-11-05', bytes: total, size: '174.7 MB' }))
    let finish: (status: unknown) => void = () => {}
    reply('pack_download', () => new Promise((resolve) => (finish = resolve)))
    await walkTo('Course pack')
    await expectText('Download the Spanish course pack (174.7 MB)?')
    fireEvent.click(screen.getByRole('button', { name: 'Download' }))
    await waitFor(() => expect(callsOf('pack_download')).toEqual([{ lang: 'es' }]))
    expect(fake.listeners.get('pack://progress')).toHaveLength(1)
    const bar = () => screen.getByRole('progressbar', { name: 'Download progress' })

    // Before the first event the pack's size is already known.
    await expectText('Downloading the Spanish course pack…')
    await expectText('0% · 0.0 of 174.7 MB')

    emitEvent('pack://progress', progressAt(0))
    await expectText('0% · 0.0 of 174.7 MB')
    expect(bar()).toHaveAttribute('max', String(total))
    expect(bar()).toHaveAttribute('value', '0')

    emitEvent('pack://progress', progressAt(8_000_000))
    await expectText('4% · 8.0 of 174.7 MB')
    expect(bar()).toHaveAttribute('value', '8000000')

    emitEvent('pack://progress', progressAt(73_400_000))
    await expectText('42% · 73.4 of 174.7 MB')
    expect(bar()).toHaveAttribute('value', '73400000')
    expect(screen.queryByText(textOf('4% · 8.0 of 174.7 MB'))).toBeNull()

    emitEvent('pack://progress', progressAt(total))
    await expectText('100% · 174.7 of 174.7 MB')
    expect(bar()).toHaveAttribute('value', String(total))

    act(() => finish({ lang: 'es', state: 'installed', version: '2026-11-05', bytes: total, size: '174.7 MB' }))
    await expectText('The Spanish course pack is installed.')
    expect(screen.queryByRole('progressbar')).toBeNull()
    expect(fake.listeners.get('pack://progress')).toEqual([])
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

  it('a placement question into the course language shows the English and takes the answer in that language', async () => {
    // A skill the test comes back to asks one of its words again, as translate_to_target.
    const intoSpanish = {
      ...QUESTION_2,
      exercise: {
        id: 'p2',
        item_id: 'es.a1.u01.s02#l:llamarse',
        facet: 'productive',
        origin: 'template',
        type: 'translate_to_target',
        prompt_en: 'to be called',
        target: 'llamarse',
        accepted: ['llamarse'],
      },
    }
    let answered = 0
    reply('placement_answer', () => {
      answered += 1
      return answered === 1 ? { next: intoSpanish, result: null } : { next: null, result: AROUND_A1 }
    })
    await walkTo('Where to start')
    fireEvent.click(screen.getByRole('button', { name: 'Take the placement test (about 5 minutes)' }))
    await heading('Translate into English', 2)
    expect(screen.getByRole('textbox', { name: 'Your answer' })).toHaveAttribute('lang', 'en')
    fireEvent.click(screen.getByRole('button', { name: "I don't know" }))

    await expectText('Question 2')
    await heading('Translate into Spanish', 2)
    expect(screen.queryByRole('heading', { name: 'Translate into English' })).toBeNull()
    expect(screen.getByText('to be called')).toHaveAttribute('lang', 'en')
    const box = screen.getByRole('textbox', { name: 'Your answer' })
    expect(box).toHaveAttribute('lang', 'es')
    expect(box).toHaveValue('')
    fireEvent.change(box, { target: { value: 'llamarse' } })
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    await waitFor(() =>
      expect(callsOf('placement_answer')[1]).toEqual({ session_id: PLACEMENT, answer: { text: 'llamarse' } }),
    )
    await expectText("You're around A1. You'll start at Unit 1: Hello.")
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

// ---- acceptance test 18: the placement test's answer box -----------------------------------------

describe('Every new placement question puts the cursor in the answer box (acceptance test 18)', () => {
  const answerBox = () => screen.getByRole('textbox', { name: 'Your answer' })

  /** A mouse click: the web view focuses the button it clicks, which `fireEvent.click` alone does not. */
  function click(name: string) {
    const button = screen.getByRole('button', { name })
    button.focus()
    fireEvent.click(button)
  }

  async function firstQuestion() {
    await walkTo('Where to start')
    fireEvent.click(screen.getByRole('button', { name: 'Take the placement test (about 5 minutes)' }))
    await heading('Placement test')
    await expectText('Question 1')
    expect(answerBox()).toHaveFocus()
  }

  async function expectCursorInNextAnswerBox(sent: string) {
    await waitFor(() =>
      expect(callsOf('placement_answer')).toEqual([{ session_id: PLACEMENT, answer: { text: sent } }]),
    )
    await expectText('Question 2')
    await waitFor(() => expect(answerBox()).toHaveFocus())
    expect(answerBox()).toHaveValue('')
  }

  it("after I don't know, the next question's answer box has the cursor", async () => {
    await firstQuestion()
    click("I don't know")
    await expectCursorInNextAnswerBox('')
  })

  it("after Next, the next question's answer box has the cursor", async () => {
    await firstQuestion()
    fireEvent.change(answerBox(), { target: { value: 'to be' } })
    click('Next')
    await expectCursorInNextAnswerBox('to be')
  })

  it("after Enter in the answer box, the next question's answer box has the cursor", async () => {
    await firstQuestion()
    fireEvent.change(answerBox(), { target: { value: 'to be' } })
    press('Enter')
    await expectCursorInNextAnswerBox('to be')
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
    // One at the top of the screen and one at its foot (navigation.test.tsx); this is the foot's.
    const backs = screen.getAllByRole('button', { name: 'Back to my path' })
    fireEvent.click(backs[backs.length - 1])
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

// ---- acceptance test 20: match the pairs ----------------------------------------------------------

/** The two columns of a match_pairs exercise. */
function matchColumns() {
  return {
    words: screen.getByRole('group', { name: 'Words' }),
    meanings: screen.getByRole('group', { name: 'Meanings' }),
  }
}

/** A button's accessible name: its label, or its text without the key shown before it. */
function nameOf(button: HTMLElement): string {
  return button.getAttribute('aria-label') ?? squash(button.textContent).slice(1)
}

/** The names of the buttons of `group` chosen and waiting for their other half. */
function pressedIn(group: HTMLElement): string[] {
  return within(group)
    .getAllByRole('button')
    .filter((button) => button.getAttribute('aria-pressed') === 'true')
    .map(nameOf)
}

describe('Match the pairs (acceptance test 20)', () => {
  beforeEach(() => {
    onboarded()
    reply('lesson_start', () => lesson([MATCH]))
  })

  it('numbers the words 1 to 4 and letters the meanings A to D, in the order the lesson gives', async () => {
    await startLesson()
    await expectText('Match the pairs')
    const { words, meanings } = matchColumns()
    const keysOf = (group: HTMLElement) =>
      within(group)
        .getAllByRole('button')
        .map((button) => button.getAttribute('aria-keyshortcuts'))
    expect(within(words).getAllByRole('button').map((button) => button.textContent)).toEqual([
      '1hola',
      '2adiós',
      '3gracias',
      '4noche',
    ])
    expect(within(meanings).getAllByRole('button').map((button) => button.textContent)).toEqual([
      'Athank you',
      'Bhello',
      'Cnight',
      'Dgoodbye',
    ])
    expect(keysOf(words)).toEqual(['1', '2', '3', '4'])
    expect(keysOf(meanings)).toEqual(['A', 'B', 'C', 'D'])
    // The accessible names stay the words themselves.
    expect(within(words).getByRole('button', { name: 'hola' })).toBeInTheDocument()
    expect(within(meanings).getByRole('button', { name: 'thank you' })).toBeInTheDocument()
  })

  it('never shows the meanings in the order of the words, even when the lesson gives no order', async () => {
    const unordered: Record<string, unknown> = { ...MATCH }
    delete unordered.english_order
    reply('lesson_start', () => lesson([unordered]))
    await startLesson()
    await expectText('Match the pairs')
    const { meanings } = matchColumns()
    const shown = within(meanings).getAllByRole('button').map(nameOf)
    expect([...shown].sort()).toEqual(['goodbye', 'hello', 'night', 'thank you'])
    expect(shown).not.toEqual(MATCH.pairs.map(([, english]) => english))
  })

  it('a number then a letter, or a letter then a number, makes a pair; Enter sends them', async () => {
    await startLesson()
    await expectText('Match the pairs')
    const { words, meanings } = matchColumns()

    press('1')
    await waitFor(() => expect(pressedIn(words)).toEqual(['hola']))
    press('5')
    expect(pressedIn(words), 'there is no word 5').toEqual(['hola'])
    press('b')
    await waitFor(() => expect(screen.getByRole('button', { name: 'hola, pair 1' })).toBeDisabled())
    expect(screen.getByRole('button', { name: 'hello, pair 1' })).toBeDisabled()
    expect(pressedIn(words)).toEqual([])

    press('C')
    await waitFor(() => expect(pressedIn(meanings)).toEqual(['night']))
    press('4')
    await waitFor(() => expect(screen.getByRole('button', { name: 'night, pair 4' })).toBeDisabled())
    expect(screen.getByRole('button', { name: 'noche, pair 4' })).toBeDisabled()

    press('2')
    press('d')
    press('a')
    press('3')
    await waitFor(() => expect(screen.getByRole('button', { name: 'thank you, pair 3' })).toBeDisabled())
    expect(screen.getByRole('button', { name: 'goodbye, pair 2' })).toBeDisabled()
    expect(callsOf('exercise_submit')).toHaveLength(0)

    press('Enter')
    await waitFor(() => expect(callsOf('exercise_submit')).toHaveLength(1))
    const sent = callsOf('exercise_submit')[0].answer as { pairs: number[][] }
    expect([...sent.pairs].map((pair) => pair.join(',')).sort()).toEqual(['0,0', '1,1', '2,2', '3,3'])
  })

  it('with the mouse, a meaning then a word makes a pair too', async () => {
    await startLesson()
    fireEvent.click(await screen.findByRole('button', { name: 'night' }))
    fireEvent.click(screen.getByRole('button', { name: 'noche' }))
    expect(await screen.findByRole('button', { name: 'noche, pair 4' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'night, pair 4' })).toBeDisabled()
  })

  it('Esc or Backspace clears a half-made pair; Esc with none asks to quit', async () => {
    await startLesson()
    await expectText('Match the pairs')
    const { words, meanings } = matchColumns()

    press('2')
    await waitFor(() => expect(pressedIn(words)).toEqual(['adiós']))
    press('Escape')
    await waitFor(() => expect(pressedIn(words)).toEqual([]))
    expectNoDialog()

    press('a')
    await waitFor(() => expect(pressedIn(meanings)).toEqual(['thank you']))
    press('Backspace')
    await waitFor(() => expect(pressedIn(meanings)).toEqual([]))
    expectNoDialog()

    // Only the half-made pair is let go: a made pair stays.
    press('1')
    press('b')
    await screen.findByRole('button', { name: 'hola, pair 1' })
    press('Backspace')
    expect(screen.getByRole('button', { name: 'hola, pair 1' })).toBeInTheDocument()

    press('Escape')
    const dialog = await screen.findByRole('dialog')
    await expectText('Quit this lesson?', dialog)
  })

  it('nothing moves when a pair is made: Start over is there from the start, under the columns', async () => {
    await startLesson()
    await expectText('Match the pairs')
    const { words, meanings } = matchColumns()
    const startOver = screen.getByRole('button', { name: 'Start over' })
    expect(startOver).toBeDisabled()
    const row = words.parentElement as HTMLElement
    expect(startOver.parentElement, 'Start over is not in the row of the columns').not.toBe(row)
    const columns = [...row.children]
    expect(columns).toHaveLength(2)
    expect(columns[0]).toBe(words)
    expect(columns[1]).toBe(meanings)
    const wordButtons = within(words).getAllByRole('button')
    const meaningButtons = within(meanings).getAllByRole('button')

    fireEvent.click(screen.getByRole('button', { name: 'gracias' }))
    fireEvent.click(screen.getByRole('button', { name: 'thank you' }))
    await screen.findByRole('button', { name: 'gracias, pair 3' })

    expect(screen.getByRole('button', { name: 'Start over' })).toBe(startOver)
    expect(startOver).toBeEnabled()
    expect(screen.getByRole('group', { name: 'Words' })).toBe(words)
    expect(screen.getByRole('group', { name: 'Meanings' })).toBe(meanings)
    expect(row.children).toHaveLength(2)
    expect(row.children[0]).toBe(words)
    expect(row.children[1]).toBe(meanings)
    within(words)
      .getAllByRole('button')
      .forEach((button, index) => expect(button).toBe(wordButtons[index]))
    within(meanings)
      .getAllByRole('button')
      .forEach((button, index) => expect(button).toBe(meaningButtons[index]))

    fireEvent.click(startOver)
    await waitFor(() => expect(screen.getByRole('button', { name: 'Start over' })).toBeDisabled())
    expect(screen.getByRole('button', { name: 'Start over' })).toBe(startOver)
    expect(screen.getByRole('button', { name: 'gracias' })).toBeEnabled()
  })

  it('a made pair shows its number and colour on both halves and stays readable', async () => {
    await startLesson()
    fireEvent.click(await screen.findByRole('button', { name: 'gracias' }))
    fireEvent.click(screen.getByRole('button', { name: 'thank you' }))
    const word = await screen.findByRole('button', { name: 'gracias, pair 3' })
    const meaning = screen.getByRole('button', { name: 'thank you, pair 3' })
    for (const half of [word, meaning]) {
      expect(within(half).getByText('3')).toBeInTheDocument()
      expect(half).toHaveClass('matched', 'pair-3')
    }
    // The meaning's letter gives way to its word's number; the others keep theirs.
    expect(within(meaning).queryByText('A')).toBeNull()
    expect(within(screen.getByRole('button', { name: 'hello' })).getByText('B')).toBeInTheDocument()
    const css = STYLESHEET.replace(/\/\*[\s\S]*?\*\//g, '')
    expect(rule(css, 'button.matched:disabled').get('opacity'), 'a made pair is not faded').toBe('1')
    for (const n of [1, 2, 3, 4, 5]) {
      expect(rule(css, `.pair-${n}`).get('--pair'), `pair ${n} has its colour`).toBe(`var(--pair-${n})`)
    }
  })

  it('the footer names the keys of the exercise shown', async () => {
    reply('lesson_start', () => lesson([MATCH, PICK_1]))
    await startLesson()
    await expectText('Press a number (1 to 4) and a letter (A to D) to make a pair.')
    await expectText('Backspace or Esc clears a half-made pair; otherwise Esc quits.')
    expect(screen.queryByText(textOf('Keys 1 to 4 choose an option.'))).toBeNull()
    for (const [target, english] of MATCH.pairs) {
      fireEvent.click(screen.getByRole('button', { name: target }))
      fireEvent.click(screen.getByRole('button', { name: english }))
    }
    press('Enter')
    await screen.findByRole('region', { name: 'Result' })
    press('Enter')
    await expectText('What does hola mean?')
    await expectText('Keys 1 to 4 choose an option.')
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
    // Continue is the lesson's action bar's (acceptance test 21), under the sheet.
    expect(screen.getByRole('button', { name: 'Continue' })).toBeInTheDocument()
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
    await screen.findByRole('region', { name: 'Result' })
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }))
    await waitFor(() => expect(callsOf('lesson_finish')).toEqual([{ session_id: SESSION }]))
    await heading('Lesson complete')
    await expectText('+17 XP')
    await expectText('2 of 2 correct')
    // No answer was left Not checked, so the summary has no line for them.
    expect(screen.queryAllByText(textOf('not checked'))).toHaveLength(0)
    await expectText('5 day streak')
    await expectText('New weekly record!')
    expect(screen.getByRole('button', { name: 'Continue' })).toBeInTheDocument()
  })
})

// ---- acceptance test 22: a free translation not checked ---------------------------------------

describe('A free translation not checked (acceptance test 22)', () => {
  beforeEach(() => {
    onboarded()
  })

  const TRANSLATE_TARGET = {
    id: 'e1',
    item_id: 'es.a1.u02.s02#e:2',
    facet: 'productive',
    origin: 'template',
    type: 'translate_to_target',
    prompt_en: 'I want a beer.',
    target: 'Quiero una cerveza.',
    accepted: ['Quiero una cerveza.', 'Quiero una cerveza, por favor.'],
  }

  /** The summary of a run of two: one right answer and one left Not checked. */
  const ONE_NOT_CHECKED = {
    ...SUMMARY,
    xp: 11,
    correct: 1,
    checked: 1,
    unchecked: 1,
    total: 2,
    record_week: false,
    message: null,
  }

  /** Types `answer` in the answer box and checks it; the result sheet. */
  async function checkTyped(answer: string) {
    fireEvent.change(await screen.findByRole('textbox', { name: 'Your answer' }), {
      target: { value: answer },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    return screen.findByRole('region', { name: 'Result' })
  }

  /**
   * Enter on the result sheet, once the graded answer has settled: Continue is enabled and the
   * lesson's keyboard handler has caught up with the grade (its effect runs after the render).
   */
  async function enterOnTheSheet() {
    await waitFor(() => expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled())
    await act(async () => {})
    press('Enter')
  }

  const SHEETS = [
    ['translate_to_en', TRANSLATE_EN, 'Morning, everyone.', 'Good morning.'],
    ['translate_to_target', TRANSLATE_TARGET, 'Yo quiero una cerveza.', 'Quiero una cerveza.'],
  ] as const

  for (const [type, exercise, answer, first] of SHEETS) {
    it(`a Not checked ${type} sheet shows the first accepted answer under the learner's own`, async () => {
      reply('lesson_start', () => lesson([exercise]))
      reply('exercise_submit', () => grade('unchecked', { rating: null }))
      await startLesson()
      const result = await checkTyped(answer)
      await expectText('Not checked', result)
      await expectText(`A correct answer: ${first}`, result)
      // Only the first accepted answer is shown, and not as the corrected answer of a verdict.
      expect(within(result).queryAllByText(textOf('por favor'))).toHaveLength(0)
      expect(within(result).queryAllByText(textOf('Correct answer:'))).toHaveLength(0)
      // The learner's own answer stays in the answer box above the sheet.
      const box = screen.getByRole('textbox', { name: 'Your answer' })
      expect(box).toHaveValue(answer)
      const line = within(result).getAllByText(textOf(`A correct answer: ${first}`))[0]
      expect(
        box.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING,
        "the correct answer comes after the learner's answer",
      ).toBeTruthy()
      // The sheet still offers I was right and Check with AI.
      expect(within(result).getByRole('button', { name: 'I was right' })).toBeInTheDocument()
      expect(within(result).getByRole('button', { name: 'Check with AI' })).toBeInTheDocument()
    })
  }

  it('Enter on a Not checked sheet continues, and the summary counts the answer as not checked, not wrong', async () => {
    reply('lesson_start', () => lesson([TRANSLATE_EN, PICK_2]))
    reply('exercise_submit', (args) =>
      args.exercise_id === 'e1' ? grade('unchecked', { rating: null }) : grade('correct'),
    )
    reply('lesson_finish', () => ONE_NOT_CHECKED)
    await startLesson()
    const result = await checkTyped('Morning, everyone.')
    await expectText('A correct answer: Good morning.', result)
    await enterOnTheSheet()
    await expectText('Exercise 2 of 2')
    expect(callsOf('exercise_i_was_right'), 'Enter is Continue, not I was right').toHaveLength(0)
    fireEvent.click(await screen.findByRole('radio', { name: 'goodbye' }))
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await screen.findByRole('region', { name: 'Result' })
    await enterOnTheSheet()
    await waitFor(() => expect(callsOf('lesson_finish')).toEqual([{ session_id: SESSION }]))
    await heading('Lesson complete')
    await expectText('1 of 1 correct')
    await expectText('1 not checked')
    expect(screen.queryAllByText(textOf('1 of 2 correct'))).toHaveLength(0)
  })

  it('a review shows the same sheet and the same summary', async () => {
    reply('review_start', () => ({ ...lesson([TRANSLATE_EN, PICK_2]), kind: 'review', skill_id: null }))
    reply('exercise_submit', (args) =>
      args.exercise_id === 'e1' ? grade('unchecked', { rating: null }) : grade('correct'),
    )
    reply('lesson_finish', () => ({ ...ONE_NOT_CHECKED, xp: 6 }))
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Review (3)' }))
    await expectText('Translate into English')
    const result = await checkTyped('Morning, everyone.')
    await expectText('Not checked', result)
    await expectText('A correct answer: Good morning.', result)
    await enterOnTheSheet()
    await expectText('Exercise 2 of 2')
    fireEvent.click(await screen.findByRole('radio', { name: 'goodbye' }))
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await screen.findByRole('region', { name: 'Result' })
    await enterOnTheSheet()
    await waitFor(() => expect(callsOf('lesson_finish')).toEqual([{ session_id: SESSION }]))
    await expectText('1 of 1 correct')
    await expectText('1 not checked')
  })
})

// ---- acceptance test 21: Check and Continue stay in one place -----------------------------------

describe('Check and Continue stay in one place (acceptance test 21)', () => {
  beforeEach(() => {
    onboarded()
  })

  const FILL_2 = { ...FILL, id: 'e2' }
  const PICK_3 = { ...PICK_2, id: 'e3' }

  /** The element that holds the answer's main button, Check or Continue. */
  function barOf(name: 'Check' | 'Continue'): HTMLElement {
    return screen.getByRole('button', { name }).parentElement as HTMLElement
  }

  it('Check, then Continue, are the last of the same action bar after a one-line and a many-line result; Enter still continues', async () => {
    reply('lesson_start', () =>
      lesson([PICK_1, FILL_2, PICK_3], [{ exercise_id: 'e2', author: 'fixture_a' }]),
    )
    reply('exercise_submit', (args) =>
      args.exercise_id === 'e2'
        ? grade('wrong', { message: 'Hola starts with an H.', corrected: 'Hola', rating: 1 })
        : grade('correct'),
    )
    await startLesson()
    await expectText('Exercise 1 of 3')
    const main = screen.getByRole('main')
    const bar = barOf('Check')
    expect(bar).toHaveClass('action-bar')
    expect(bar.lastElementChild, 'Check is the last of the bar').toBe(
      screen.getByRole('button', { name: 'Check' }),
    )
    expect(main.lastElementChild, 'the bar is the last of the lesson').toBe(bar)
    // The line naming the keys sits in the bar, before its button.
    expect(within(bar).queryAllByText(textOf('Enter checks and continues.'))).toHaveLength(1)

    // A one-line result: the verdict and nothing else.
    fireEvent.click(screen.getByRole('radio', { name: 'hello' }))
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const short = await screen.findByRole('region', { name: 'Result' })
    await expectText('Correct', short)
    expect(short.children, 'the sheet holds its verdict only').toHaveLength(1)
    const first = within(bar).getByRole('button', { name: 'Continue' })
    expect(bar.lastElementChild, 'Continue takes the place of Check').toBe(first)
    expect(within(bar).queryByRole('button', { name: 'Check' })).toBeNull()
    expect(short).not.toContainElement(first)
    expect(
      short.compareDocumentPosition(bar) & Node.DOCUMENT_POSITION_FOLLOWING,
      'the bar comes after the sheet',
    ).toBeTruthy()
    expect(main.lastElementChild).toBe(bar)

    fireEvent.click(first)
    await expectText('Exercise 2 of 3')
    expect(barOf('Check'), 'the next exercise keeps the same bar').toBe(bar)

    // A many-line result: the verdict, the message, the correct answer, the attribution, Explain.
    fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: 'Ola' } })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const long = await screen.findByRole('region', { name: 'Result' })
    await expectText('Hola starts with an H.', long)
    await expectText('Correct answer: Hola', long)
    await expectText('Sentence by fixture_a on Tatoeba', long)
    expect(long.children.length, 'the sheet runs to several lines').toBeGreaterThanOrEqual(5)
    expect(within(long).getByRole('button', { name: 'Explain' })).toBeInTheDocument()
    expect(within(long).queryByRole('button', { name: 'Continue' })).toBeNull()
    const second = within(bar).getByRole('button', { name: 'Continue' })
    expect(barOf('Continue')).toBe(bar)
    expect(bar.lastElementChild).toBe(second)
    expect(screen.getAllByRole('button', { name: 'Continue' })).toEqual([second])
    expect(main.lastElementChild).toBe(bar)

    press('Enter')
    await expectText('Exercise 3 of 3')
    expect(barOf('Check')).toBe(bar)
    expect(callsOf('exercise_submit')).toHaveLength(2)
  })

  it('the sheet keeps its own buttons; while Checking… shows, the bar keeps Check, disabled', async () => {
    reply('lesson_start', () => lesson([TRANSLATE_EN]))
    let release: (value: unknown) => void = () => {}
    reply(
      'exercise_submit',
      () =>
        new Promise((resolve) => {
          release = resolve
        }),
    )
    await startLesson()
    await expectText('Translate into English')
    const bar = barOf('Check')
    fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: 'Morning, everyone.' } })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const checking = await screen.findByRole('region', { name: 'Result' })
    await expectText('Checking…', checking)
    expect(within(checking).queryAllByRole('button')).toHaveLength(0)
    expect(within(bar).getByRole('button', { name: 'Check' })).toBeDisabled()
    expect(bar.lastElementChild).toBe(within(bar).getByRole('button', { name: 'Check' }))

    await act(async () => {
      release(grade('unchecked', { rating: null }))
    })
    const result = await screen.findByRole('region', { name: 'Result' })
    await expectText('Not checked', result)
    expect(within(result).getByRole('button', { name: 'I was right' })).toBeInTheDocument()
    expect(within(result).getByRole('button', { name: 'Check with AI' })).toBeInTheDocument()
    expect(within(result).queryByRole('button', { name: 'Continue' })).toBeNull()
    expect(bar.lastElementChild).toBe(within(bar).getByRole('button', { name: 'Continue' }))
    expect(within(bar).getByRole('button', { name: 'Continue' })).toBeEnabled()
  })

  it('the stylesheet keeps the bar at the foot of a lesson at least as tall as the window', () => {
    const css = STYLESHEET.replace(/\/\*[\s\S]*?\*\//g, '')
    const bar = rule(css, '.action-bar')
    expect(bar.get('position'), 'sticky: held at the foot, and the page leaves room for it').toBe('sticky')
    expect(bar.get('bottom')).toBe('0')
    expect(bar.get('margin-top'), 'pushed to the foot of a short lesson').toBe('auto')
    expect(bar.get('background'), 'nothing shows through it').toBe('var(--background)')
    const lesson = rule(css, 'main.lesson')
    expect(lesson.get('min-height')).toBe('100vh')
    expect(lesson.get('box-sizing')).toBe('border-box')
    expect(lesson.get('padding-bottom'), "the bar's foot is the lesson's").toBe('0')
    expect(
      rule(css, 'html:has(main.lesson)').get('scrollbar-gutter'),
      'a scrollbar coming in does not move the column sideways',
    ).toBe('stable')
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

  // Acceptance test 17: never half a delete. With the data file in use, nothing changes and the
  // message shows under the button that was pressed.
  const DELETE_IN_USE =
    'Parley could not delete your progress because another program is using it. Close it and try again.'
  const RESTORE_IN_USE =
    'Parley could not replace your progress because another program is using it. Close it and try again.'

  const follows = (first: Node, second: Node) =>
    (first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0

  it('Delete everything with the data file in use shows why under the button and stays on Data', async () => {
    reply('data_delete_all', failWith(appError('busy', DELETE_IN_USE)))
    await openSettings('Data')
    fireEvent.change(screen.getByRole('textbox', { name: 'Type delete to confirm.' }), {
      target: { value: 'delete' },
    })
    const launches = callsOf('app_start').length
    const button = screen.getByRole('button', { name: 'Delete everything' })
    fireEvent.click(button)
    const message = await screen.findByText(DELETE_IN_USE)
    expect(message).toHaveAttribute('role', 'status')
    expect(follows(button, message)).toBe(true)
    expect(button.parentElement).toContainElement(message)
    expect(callsOf('data_delete_all')).toEqual([{ confirm_text: 'delete' }])
    expect(callsOf('app_start')).toHaveLength(launches)
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Welcome to Parley' })).toBeNull()
  })

  it('Replace my data with the data file in use shows why under Restore a backup', async () => {
    reply('file_pick', () => 'backups/parley-backup.json')
    reply('backup_import', failWith(appError('busy', RESTORE_IN_USE)))
    await openSettings('Data')
    const launches = callsOf('app_start').length
    const restore = screen.getByRole('button', { name: 'Restore a backup' })
    fireEvent.click(restore)
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Replace my data' }))
    const message = await screen.findByText(RESTORE_IN_USE)
    expect(message).toHaveAttribute('role', 'status')
    expect(follows(restore, message)).toBe(true)
    // Straight under the Data buttons, before the course packs and Delete everything.
    expect(follows(message, screen.getByRole('heading', { name: 'Course packs' }))).toBe(true)
    expect(follows(message, screen.getByRole('button', { name: 'Delete everything' }))).toBe(true)
    expect(callsOf('backup_import')).toEqual([{ src_path: 'backups/parley-backup.json' }])
    expect(callsOf('app_start')).toHaveLength(launches)
    expectNoDialog()
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

// ---- dialogs sit above everything ----------------------------------------------------------------

/** The declarations of the first rule whose selector is exactly `selector`, by property. */
function rule(css: string, selector: string): Map<string, string> {
  const found = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find((match) => match[1].trim() === selector)
  expect(found, `a rule for ${selector}`).toBeDefined()
  const declarations = new Map<string, string>()
  for (const declaration of (found?.[2] ?? '').split(';')) {
    const colon = declaration.indexOf(':')
    if (colon > 0) declarations.set(declaration.slice(0, colon).trim(), declaration.slice(colon + 1).trim())
  }
  return declarations
}

/** A z-index value as a number: an integer, or a custom property of `:root` that holds one. */
function zIndex(css: string, value: string): number {
  const property = /^var\(\s*(--[\w-]+)\s*\)$/.exec(value)
  const resolved = property ? rule(css, ':root').get(property[1]) : value
  expect(resolved, `${value} is an integer`).toMatch(/^-?\d+$/)
  return Number(resolved)
}

describe('Dialogs sit above everything (Design notes)', () => {
  it('the dialog backdrop sets a z-index above every other z-index in the stylesheet', () => {
    expect(STYLESHEET, 'app/src/index.css is read as text').toContain('.dialog-backdrop')
    const css = STYLESHEET.replace(/\/\*[\s\S]*?\*\//g, '')
    const backdrop = rule(css, '.dialog-backdrop')
    expect(backdrop.get('position')).toBe('fixed')
    const set = backdrop.get('z-index')
    expect(set, '.dialog-backdrop sets a z-index').toBeDefined()
    const layer = zIndex(css, set ?? '')
    expect(layer).toBeGreaterThan(0)
    // The backdrop's own z-index is the only one at the dialogs' layer or above.
    const all = [...css.matchAll(/z-index\s*:\s*([^;}]+)/g)].map((match) => zIndex(css, match[1].trim()))
    expect(all.filter((value) => value >= layer), "z-index values at the dialogs' layer or above").toEqual([layer])
  })
})
