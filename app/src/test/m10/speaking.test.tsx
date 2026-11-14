// Module 10 UI acceptance tests (docs/modules/10-speaking-and-pronunciation.md, UI; the UI halves of
// acceptance tests 11, 14, 15, 16, 19, 21 and 25): the speak exercise (hold to talk with Space or
// the button, the level meter, the take, Check, what Parley heard, Play the voice, Try again),
// Can't speak now, the errors of a check, and Settings > Speaking (the models, their sizes and
// downloads, and a build without local Whisper). The app is rendered whole; the Tauri bridge
// answers from a table of the spec's result shapes (Recording, SpeakResult, ModelInfo,
// SpeakingStatus, AppError) and records every call with the snake_case arguments the spec gives;
// events (mic://level, mic://limit, model://progress) are delivered to the handlers the app
// registered. Every element is found by its role and the visible text or label the spec names.
//
// Where the spec leaves a detail open, these tests assume that Space is listened for on the
// document (so a key pressed with the page focused reaches it), that the result's word list and
// score are shown on the lesson screen once a check returns, that Try again removes the take and
// the result, and that AI provider, Local model and Use {name} are buttons or radio buttons.

import { act, cleanup, createEvent, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

type Args = Record<string, unknown>

/** What a Tauri event handler receives. */
type TauriEvent = { event: string; id: number; payload: unknown }

const fake = vi.hoisted(() => ({
  answers: new Map<string, (args: Record<string, unknown>) => unknown>(),
  calls: [] as { command: string; args: Record<string, unknown> }[],
  /** The handlers `listen` registered, by event name, until they unlisten. */
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

// Every handler is kept, so a test can deliver an event the core would emit.
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

// ---- the spec's texts --------------------------------------------------------------------------

const HOLD = 'Hold to talk, or hold Space'
const TOO_QUIET = "We didn't hear anything. Try again a little closer to the microphone."
const OFFLINE_NOTICE = "You're offline, so the local model checked this."
const BALANCE = 'Your publik balance is $0.00. Add a plan or pack to keep using AI features.'
const CANT_CHECK = "Can't check speaking right now."
const TOP_UP = 'https://publikhq.com/wallet/add-credit?from=402'
const BUSY = 'publik API is busy; try again in a moment'
const NOT_SET_UP = 'AI features are not set up yet.'
const COST_LINE = 'Speaking checks cost about $0.00054 a take (measured).'
const NOT_IN_BUILD = 'Local speech checks are not available in this build.'
const KEEP_NOTE = 'Recordings stay on this computer.'
const LEVEL = 'mic://level'
const LIMIT = 'mic://limit'
const PROGRESS = 'model://progress'
/** Can't speak now, with either apostrophe. */
const CANT_SPEAK = /can.t speak now/i
/** The 402 link as AiBanner shows it. */
const LINKS = ['Sign in with publik', 'Add a plan or pack']

// ---- the spec's result shapes ------------------------------------------------------------------

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

const STATUS_PUBLIK = {
  ...STATUS_UNSET,
  provider: 'publik',
  needs_consent: false,
  claim_state: 'claimed',
  balance_micros: 3_120_000,
  balance_line: '$3.12 of publik balance',
  add_credit_url: 'https://publikhq.com/wallet/add-credit',
  top_up_url: 'https://publikhq.com/wallet/top-up',
}

const SPEAKING = {
  enabled: true,
  transcriber: 'provider',
  pass_threshold: 0.8,
  keep_recordings: false,
  whisper_model: 'base',
}

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
  speaking: SPEAKING,
  ai: {
    provider: 'publik',
    fresh_lessons: true,
    prepare_next_lesson: true,
    stories: true,
    share_device_name: false,
    own_key: { base_url: null, chat_model: null, transcribe_model: null, speech_model: null, speech_voice: null },
    local: { base_url: 'http://localhost:11434/v1', chat_model: null },
  },
  update_check: true,
}

/** The settings with some speaking settings changed. */
function settingsWith(speaking: Record<string, unknown>) {
  return { ...SETTINGS, speaking: { ...SPEAKING, ...speaking } }
}

const COURSES = [
  { lang: 'es', name: 'Spanish', started: true, active: true, placement_done: true, cefr_estimate: null, skills: 8 },
]

const PATH = {
  lang: 'es',
  units: [
    {
      id: 'es.a1.u02',
      n: 2,
      title: 'People',
      cefr: 'A1',
      skills: [
        {
          id: 'es.a1.u02.s01',
          title: 'Who I am',
          can_do: 'I can say who I am.',
          cefr: 'A1',
          state: 'available',
          mastery: 0,
          tested_out: false,
        },
      ],
      story: null,
    },
  ],
  next_skill_id: 'es.a1.u02.s01',
  due: 0,
  streak: 4,
  freezes: 1,
  today_xp: 12,
  goal_xp: 20,
}

const SESSION = 's-0000000000000010'
const SENTENCE = 'Yo soy estudiante.'

/** A speak exercise: its text and its English. */
const SPEAK = {
  id: 'e1',
  item_id: 'es.a1.u02.s01#e:1',
  facet: 'productive',
  origin: 'template',
  type: 'speak',
  text: SENTENCE,
  prompt_en: 'I am a student.',
}

const SPEAK_TOO = {
  id: 'e2',
  item_id: 'es.a1.u02.s01#e:2',
  facet: 'productive',
  origin: 'template',
  type: 'speak',
  text: 'Buenos días.',
  prompt_en: 'Good morning.',
}

/** A pick_meaning exercise after the speaking ones. */
const PICK_LAST = {
  id: 'e3',
  item_id: 'es.a1.u02.s01#l:café',
  facet: 'receptive',
  origin: 'template',
  type: 'pick_meaning',
  prompt_target: 'el café',
  options: ['coffee', 'tea', 'water', 'milk'],
  answer_index: 0,
}

function lesson(exercises: object[]) {
  return {
    session_id: SESSION,
    kind: 'lesson',
    skill_id: 'es.a1.u02.s01',
    origin: 'template',
    exercises,
    attributions: [],
    notice: null,
    ai_error: null,
    story: null,
  }
}

function grade(verdict: string, extra: Record<string, unknown> = {}) {
  return { verdict, tags: [], message: null, corrected: null, words: [], rating: null, ...extra }
}

function appError(code: string, message: string, action: Record<string, unknown> | null = null) {
  return { code, message, action }
}

/** SpeakResult: one word right, one heard as another, one missed, and one extra word. */
const RESULT = {
  words: [
    { expected: 'Yo', heard: 'yo', status: 'ok' },
    { expected: 'soy', heard: 'es', status: 'substituted' },
    { expected: 'estudiante', heard: null, status: 'missed' },
  ],
  extra: ['señor'],
  score: 1 / 3,
  pass: false,
  heard: 'yo es señor',
  grade: grade('wrong', { corrected: SENTENCE, words: ['soy', 'estudiante'], rating: 1 }),
  rated: true,
  transcriber: 'provider',
  cached: false,
  notice: null,
}

const SPEAKING_STATUS = { cost_line: COST_LINE, speak_off_until: null, local_available: true, microphone: true }

/** ModelInfo of the bundled registry, nothing downloaded, base chosen. */
const MODELS = [
  { name: 'tiny', bytes: 77_691_713, size: '77.7 MB', downloaded: false, chosen: false },
  { name: 'base', bytes: 147_951_465, size: '148.0 MB', downloaded: false, chosen: true },
  { name: 'small', bytes: 487_601_967, size: '487.6 MB', downloaded: false, chosen: false },
]

// ---- the fake bridge ------------------------------------------------------------------------------

function reply(command: string, answer: (args: Args) => unknown) {
  fake.answers.set(command, answer)
}

function callsOf(command: string): Args[] {
  return fake.calls.filter((call) => call.command === command).map((call) => call.args)
}

/** Delivers `payload` to every handler registered for `event`, as the core's emit would. */
function emitEvent(event: string, payload: unknown) {
  const delivered: TauriEvent = { event, id: 1, payload }
  act(() => {
    for (const handler of fake.listeners.get(event) ?? []) {
      handler(delivered)
    }
  })
}

/** Waits until the app listens for `event`. */
async function listening(event: string) {
  await waitFor(() => expect(fake.listeners.get(event)?.length ?? 0).toBeGreaterThan(0))
}

let aiStatus: Record<string, unknown> = STATUS_PUBLIK

/** `base` with `patch` merged in, key by key, as the settings patch of module 04 is applied. */
function merged(base: Record<string, unknown>, patch: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base }
  for (const [key, value] of Object.entries(patch)) {
    const kept = out[key]
    out[key] =
      value && typeof value === 'object' && !Array.isArray(value) && kept && typeof kept === 'object'
        ? merged(kept as Record<string, unknown>, value as Record<string, unknown>)
        : value
  }
  return out
}

/** Each mic_stop gives the next take: rec-1, rec-2, …, 1.5 s each. */
let takes = 0

function answerDefaults() {
  fake.answers.clear()
  fake.calls.length = 0
  fake.listeners.clear()
  aiStatus = STATUS_PUBLIK
  takes = 0
  reply('app_start', () => ({ onboarded: true, active_course: 'es', notice: null }))
  reply('app_info', () => ({ version: '9.9.9', data_dir: 'parley-data-folder' }))
  reply('settings_get', () => SETTINGS)
  reply('settings_set', (args) => merged(SETTINGS, args.patch as Record<string, unknown>))
  reply('course_list', () => COURSES)
  reply('pack_status', () => ({ lang: 'es', state: 'installed', version: '2026-10-01', bytes: 8_200_000, size: '8.2 MB' }))
  reply('path_get', () => PATH)
  reply('stats_get', () => ({ lang: 'es', empty: true, streak: 0, freezes: 0, days: [] }))
  reply('ai_status', () => aiStatus)
  reply('ai_lesson_checks', () => ({ lessons: 0, valid: 0, planned: 0, message: null }))
  reply('ai_consent_decline', () => aiStatus)
  reply('ai_open_link', () => null)
  reply('ai_reconnect_publik', () => aiStatus)
  reply('lesson_start', () => lesson([SPEAK]))
  reply('exercise_submit', () => grade('correct', { rating: 3 }))
  reply('lesson_finish', () => ({
    xp: 1,
    correct: 1,
    total: 1,
    streak: 5,
    freezes: 1,
    record_week: false,
    message: null,
    daily_xp: 13,
    goal_xp: 20,
  }))
  // Module 09.
  reply('voices_list', () => [])
  reply('tts_play', () => ({ source: 'os', cached: false, notice: null, ai_error: null }))
  reply('tts_stop', () => null)
  reply('tts_prefetch', () => ({ prefetched: false }))
  reply('audio_status', () => ({ cost_line: 'Natural voices cost about $0.00054 a sentence (measured).', listen_off_until: null }))
  reply('session_skip', () => null)
  // Module 10.
  reply('mic_start', () => null)
  reply('mic_stop', () => {
    takes += 1
    return { id: `rec-${takes}`, duration_ms: 1_500 }
  })
  reply('recording_play', () => null)
  reply('speak_check', () => RESULT)
  reply('speaking_status', () => SPEAKING_STATUS)
  reply('whisper_models', () => MODELS)
  reply('whisper_download', () => MODELS)
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

function hasText(text: string, container: HTMLElement = document.body) {
  return within(container).queryAllByText(textOf(text)).length > 0
}

/** Buttons and radio buttons named `name`. */
function controls(name: string | RegExp): HTMLElement[] {
  return [...screen.queryAllByRole('button', { name }), ...screen.queryAllByRole('radio', { name })]
}

/** A button or radio button named `name`. */
async function findControl(name: string | RegExp): Promise<HTMLElement> {
  return waitFor(() => {
    const found = controls(name)
    expect(found.length, `a control named ${name}`).toBeGreaterThan(0)
    return found[0]
  })
}

/** Lets pending promises and effects run, so a call that should not happen would have happened. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50))
  })
}

/** Opens the path and starts the next lesson with Continue. */
async function startLesson() {
  render(<App />)
  await expectText('4 day streak')
  fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
}

function holdButton(): HTMLElement {
  return screen.getByRole('button', { name: HOLD })
}

/** Holds the button until the capture has started, then lets go; returns once the take shows. */
async function recordTake() {
  const starts = callsOf('mic_start').length
  const stops = callsOf('mic_stop').length
  fireEvent.pointerDown(await screen.findByRole('button', { name: HOLD }))
  await waitFor(() => expect(callsOf('mic_start')).toHaveLength(starts + 1))
  await waitFor(() => expect(holdButton().textContent).toContain('Recording…'))
  fireEvent.pointerUp(holdButton())
  await waitFor(() => expect(callsOf('mic_stop')).toHaveLength(stops + 1))
  await expectText('Your take: 1.5 s')
}

/** Starts a speak lesson, records one take and checks it. */
async function checkedTake() {
  await startLesson()
  await recordTake()
  fireEvent.click(screen.getByRole('button', { name: 'Check' }))
  await waitFor(() => expect(callsOf('speak_check').length).toBeGreaterThan(0))
}

function spaceDown(repeat = false) {
  fireEvent.keyDown(document.body, { key: ' ', code: 'Space', repeat })
}

function spaceUp() {
  fireEvent.keyUp(document.body, { key: ' ', code: 'Space' })
}

async function openSpeakingSettings() {
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
  await screen.findByRole('heading', { name: 'Settings' })
  fireEvent.click(await screen.findByRole('button', { name: 'Speaking' }))
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  cleanup()
})

// ---- acceptance test 25: the speak exercise -------------------------------------------------

describe('The speak exercise (acceptance test 25)', () => {
  it('shows Say this, the sentence, its English, Play and Slow, and Hold to talk', async () => {
    await startLesson()
    expect(await screen.findByRole('heading', { name: 'Say this' })).toBeInTheDocument()
    await expectText(SENTENCE)
    await expectText('I am a student.')
    expect(screen.getByRole('button', { name: 'Play the sentence' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Play it slowly' })).toBeInTheDocument()
    const hold = await screen.findByRole('button', { name: HOLD })
    expect(hold.textContent).toContain('Hold to talk')
    expect(callsOf('mic_start'), 'nothing is recorded until the learner holds').toHaveLength(0)
  })

  it('Space down and up call mic_start {max_s: 15} and mic_stop {}, and the take shows', async () => {
    await startLesson()
    await screen.findByRole('button', { name: HOLD })
    const down = createEvent.keyDown(document.body, { key: ' ', code: 'Space' })
    fireEvent(document.body, down)
    await waitFor(() => expect(callsOf('mic_start')).toEqual([{ max_s: 15 }]))
    expect(down.defaultPrevented, 'Space does not scroll or press a button').toBe(true)
    await waitFor(() => expect(holdButton().textContent).toContain('Recording…'))

    spaceDown(true)
    await settle()
    expect(callsOf('mic_start'), 'a repeat is not a new take').toHaveLength(1)

    spaceUp()
    await waitFor(() => expect(callsOf('mic_stop')).toEqual([{}]))
    await expectText('Your take: 1.5 s')
    expect(await screen.findByRole('button', { name: 'Replay my take' })).toBeInTheDocument()
  })

  it("the button's pointer down and up call mic_start and mic_stop, showing Recording… meanwhile", async () => {
    await startLesson()
    fireEvent.pointerDown(await screen.findByRole('button', { name: HOLD }))
    await waitFor(() => expect(callsOf('mic_start')).toEqual([{ max_s: 15 }]))
    await waitFor(() => expect(holdButton().textContent).toContain('Recording…'))
    fireEvent.pointerUp(holdButton())
    await waitFor(() => expect(callsOf('mic_stop')).toEqual([{}]))
    await expectText('Your take: 1.5 s')
    await waitFor(() => expect(holdButton().textContent).not.toContain('Recording…'))
  })

  it('mic://limit calls mic_stop as a release would', async () => {
    await startLesson()
    fireEvent.pointerDown(await screen.findByRole('button', { name: HOLD }))
    await waitFor(() => expect(callsOf('mic_start')).toHaveLength(1))
    await waitFor(() => expect(holdButton().textContent).toContain('Recording…'))
    await listening(LIMIT)
    emitEvent(LIMIT, {})
    await waitFor(() => expect(callsOf('mic_stop')).toEqual([{}]))
    await expectText('Your take: 1.5 s')
  })

  it('Check before any take asks for one and calls nothing', async () => {
    await startLesson()
    await screen.findByRole('button', { name: HOLD })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const alert = await screen.findByRole('alert')
    expect(squash(alert.textContent)).toContain('Hold to talk and say the sentence first.')
    await settle()
    expect(callsOf('speak_check')).toHaveLength(0)
  })

  it('Check calls speak_check with local false and shows each word, the extra words and the score', async () => {
    await checkedTake()
    expect(callsOf('speak_check')).toEqual([
      { session_id: SESSION, exercise_id: 'e1', recording_id: 'rec-1', local: false },
    ])
    await expectText('Parley heard: yo es señor')
    const list = await screen.findByRole('list', { name: 'What Parley heard' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(3)
    const expected = [
      { className: 'word-ok', icon: '✓', label: 'Yo: right' },
      { className: 'word-substituted', icon: '≠', label: 'soy: heard es' },
      { className: 'word-missed', icon: '✗', label: 'estudiante: missed' },
    ]
    expected.forEach(({ className, icon, label }, i) => {
      expect(items[i]).toHaveClass(className)
      const hidden = items[i].querySelector('[aria-hidden="true"]')
      expect(hidden, `${label} has an icon hidden from screen readers`).not.toBeNull()
      expect(squash(hidden?.textContent)).toBe(icon)
      expect(hasText(label, items[i]), label).toBe(true)
    })
    await expectText('Also heard: señor')
    // round(1/3 x 100) = 33; round(0.8 x 100) = 80.
    await expectText('Score 33% (pass mark 80%)')
    expect(await screen.findByRole('region', { name: 'Result' })).toBeInTheDocument()
    expect(callsOf('exercise_submit'), 'a spoken answer is not typed').toHaveLength(0)
  })

  it('shows no Also heard line when nothing extra was heard, and the pass mark of the settings', async () => {
    reply('settings_get', () => settingsWith({ pass_threshold: 0.6 }))
    reply('speak_check', () => ({
      ...RESULT,
      words: [
        { expected: 'Yo', heard: 'yo', status: 'ok' },
        { expected: 'soy', heard: 'es', status: 'substituted' },
        { expected: 'estudiante', heard: 'estudiante', status: 'ok' },
      ],
      extra: [],
      score: 2 / 3,
      pass: true,
      heard: 'yo es estudiante',
    }))
    await checkedTake()
    await expectText('Score 67% (pass mark 60%)')
    expect(hasText('Also heard:')).toBe(false)
  })

  it('shows the notice of a result', async () => {
    reply('speak_check', () => ({ ...RESULT, transcriber: 'local', notice: OFFLINE_NOTICE }))
    await checkedTake()
    await expectText(OFFLINE_NOTICE)
  })

  it('Play the voice calls tts_play with the sentence, not slow', async () => {
    await checkedTake()
    const before = callsOf('tts_play').length
    fireEvent.click(await screen.findByRole('button', { name: 'Play the voice' }))
    await waitFor(() => expect(callsOf('tts_play')).toHaveLength(before + 1))
    expect(callsOf('tts_play')[before]).toMatchObject({ text: SENTENCE, slow: false })
  })

  it('Try again clears the take and the result, and a new take is checked again', async () => {
    reply('speak_check', () => (callsOf('speak_check').length === 1 ? RESULT : { ...RESULT, rated: false }))
    await checkedTake()
    await screen.findByRole('list', { name: 'What Parley heard' })
    fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(screen.queryByRole('list', { name: 'What Parley heard' })).toBeNull())
    expect(hasText('Your take:')).toBe(false)

    await recordTake()
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await waitFor(() => expect(callsOf('speak_check')).toHaveLength(2))
    expect(callsOf('speak_check')[1]).toEqual({
      session_id: SESSION,
      exercise_id: 'e1',
      recording_id: 'rec-2',
      local: false,
    })
    await screen.findByRole('list', { name: 'What Parley heard' })
  })

  it('a take that was too quiet shows the message', async () => {
    reply('mic_stop', () => Promise.reject(appError('too_quiet', TOO_QUIET)))
    await startLesson()
    fireEvent.pointerDown(await screen.findByRole('button', { name: HOLD }))
    await waitFor(() => expect(callsOf('mic_start')).toHaveLength(1))
    await waitFor(() => expect(holdButton().textContent).toContain('Recording…'))
    fireEvent.pointerUp(holdButton())
    await expectText(TOO_QUIET)
    expect(screen.getAllByRole('alert').some((alert) => hasText(TOO_QUIET, alert))).toBe(true)
  })

  it('a transcript with no words shows the too-quiet message', async () => {
    reply('speak_check', () => Promise.reject(appError('too_quiet', TOO_QUIET)))
    await checkedTake()
    await expectText(TOO_QUIET)
    expect(screen.queryByRole('list', { name: 'What Parley heard' })).toBeNull()
  })
})

// ---- acceptance test 19: the level meter ------------------------------------------------------

describe('The level meter (acceptance test 19)', () => {
  it("the meter's value follows mic://level, from -60 to 0 dBFS", async () => {
    await startLesson()
    fireEvent.pointerDown(await screen.findByRole('button', { name: HOLD }))
    const meter = await screen.findByRole('meter', { name: 'Microphone level' })
    expect(meter).toHaveAttribute('aria-valuemin', '-60')
    expect(meter).toHaveAttribute('aria-valuemax', '0')
    await listening(LEVEL)
    emitEvent(LEVEL, { dbfs: -12.4 })
    await waitFor(() =>
      expect(screen.getByRole('meter', { name: 'Microphone level' })).toHaveAttribute('aria-valuenow', '-12'),
    )
    emitEvent(LEVEL, { dbfs: -30.6 })
    await waitFor(() =>
      expect(screen.getByRole('meter', { name: 'Microphone level' })).toHaveAttribute('aria-valuenow', '-31'),
    )
  })
})

// ---- acceptance test 16: Replay my take -----------------------------------------------------------

describe('Replay my take (acceptance test 16)', () => {
  it("calls recording_play with the take's id", async () => {
    await startLesson()
    await recordTake()
    fireEvent.click(await screen.findByRole('button', { name: 'Replay my take' }))
    await waitFor(() => expect(callsOf('recording_play')).toEqual([{ recording_id: 'rec-1' }]))
  })
})

// ---- acceptance test 14: Can't speak now ----------------------------------------------------------

describe("Can't speak now (acceptance test 14)", () => {
  it('calls session_skip with speak and skips this and every later speak exercise with no grade', async () => {
    reply('lesson_start', () => lesson([SPEAK, SPEAK_TOO, PICK_LAST]))
    await startLesson()
    await screen.findByRole('button', { name: HOLD })
    fireEvent.click(await findControl(CANT_SPEAK))
    await waitFor(() => expect(callsOf('session_skip')).toEqual([{ kind: 'speak' }]))
    await expectText('el café')
    expect(screen.queryByRole('button', { name: HOLD })).toBeNull()
    expect(hasText('Buenos días.'), 'the later speak exercise is skipped too').toBe(false)
    expect(callsOf('exercise_submit'), 'skipped with no grade').toHaveLength(0)
    expect(callsOf('speak_check')).toHaveLength(0)
    expect(callsOf('session_skip')).toHaveLength(1)
  })

  const closed: [string, () => void][] = [
    ['no microphone', () => reply('speaking_status', () => ({ ...SPEAKING_STATUS, microphone: false }))],
    [
      'speaking skipped for the next hour',
      () => reply('speaking_status', () => ({ ...SPEAKING_STATUS, speak_off_until: Date.now() + 3_600_000 })),
    ],
    ['speaking exercises off', () => reply('settings_get', () => settingsWith({ enabled: false }))],
  ]
  for (const [why, arrange] of closed) {
    it(`a lesson opened with ${why} skips its speak exercises with no grade`, async () => {
      arrange()
      reply('lesson_start', () => lesson([SPEAK, SPEAK_TOO, PICK_LAST]))
      await startLesson()
      await expectText('el café')
      expect(screen.queryByRole('button', { name: HOLD })).toBeNull()
      await settle()
      expect(callsOf('mic_start')).toHaveLength(0)
      expect(callsOf('speak_check')).toHaveLength(0)
      expect(callsOf('exercise_submit')).toHaveLength(0)
    })
  }
})

// ---- acceptance test 15: the errors of a check ----------------------------------------------------

describe('Errors of a check (acceptance test 15)', () => {
  it("a 402 shows the message and link, Use the local model calls speak_check with local true, and Can't speak now is offered", async () => {
    reply('speak_check', (args) =>
      args.local
        ? RESULT
        : Promise.reject(appError('needs_credit', `${CANT_CHECK} ${BALANCE}`, { top_up_url: TOP_UP, use_local: true })),
    )
    await checkedTake()
    await expectText(`${CANT_CHECK} ${BALANCE}`)
    expect(screen.getAllByRole('alert').length).toBeGreaterThan(0)
    const links = LINKS.flatMap((name) => screen.queryAllByRole('button', { name }))
    expect(links, 'the one link').toHaveLength(1)
    expect(controls(CANT_SPEAK).length).toBeGreaterThan(0)
    fireEvent.click(await screen.findByRole('button', { name: 'Use the local model' }))
    await waitFor(() => expect(callsOf('speak_check')).toHaveLength(2))
    expect(callsOf('speak_check')[1]).toEqual({
      session_id: SESSION,
      exercise_id: 'e1',
      recording_id: 'rec-1',
      local: true,
    })
    await screen.findByRole('list', { name: 'What Parley heard' })
  })

  it('a 402 without a downloaded model offers no Use the local model', async () => {
    reply('speak_check', () =>
      Promise.reject(appError('needs_credit', `${CANT_CHECK} ${BALANCE}`, { top_up_url: TOP_UP, use_local: false })),
    )
    await checkedTake()
    await expectText(`${CANT_CHECK} ${BALANCE}`)
    await settle()
    expect(screen.queryByRole('button', { name: 'Use the local model' })).toBeNull()
  })

  it('busy offers Try again, which checks the same take again', async () => {
    reply('speak_check', () =>
      callsOf('speak_check').length === 1 ? Promise.reject(appError('busy', BUSY)) : RESULT,
    )
    await checkedTake()
    await expectText(BUSY)
    fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(callsOf('speak_check')).toHaveLength(2))
    expect(callsOf('speak_check')[1]).toEqual(callsOf('speak_check')[0])
  })

  it('disconnected offers Reconnect', async () => {
    reply('speak_check', () =>
      Promise.reject(appError('disconnected', `${CANT_CHECK} publik API is disconnected.`, { use_local: false })),
    )
    await checkedTake()
    await expectText(`${CANT_CHECK} publik API is disconnected.`)
    expect(await screen.findByRole('button', { name: 'Reconnect' })).toBeInTheDocument()
  })

  it('needs_consent opens the AI setup dialog and checks again after its Continue', async () => {
    aiStatus = STATUS_UNSET
    reply('speak_check', () =>
      callsOf('speak_check').length === 1 ? Promise.reject(appError('needs_consent', NOT_SET_UP)) : RESULT,
    )
    reply('ai_consent_accept', () => {
      aiStatus = STATUS_PUBLIK
      return aiStatus
    })
    await checkedTake()
    const dialog = await screen.findByRole('dialog', { name: 'Set up AI features' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Continue with publik API' }))
    await waitFor(() => expect(callsOf('ai_consent_accept')).toHaveLength(1))
    // The first-run card of module 06 may follow; its Continue ends the setup.
    await waitFor(() => {
      const card = screen.queryByRole('dialog')
      const go = card ? within(card).queryByRole('button', { name: 'Continue' }) : null
      expect(callsOf('speak_check').length >= 2 || go !== null).toBe(true)
    })
    if (callsOf('speak_check').length < 2) {
      fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Continue' }))
    }
    await waitFor(() => expect(callsOf('speak_check')).toHaveLength(2))
    expect(callsOf('speak_check')[1]).toEqual(callsOf('speak_check')[0])
  })
})

// ---- Settings > Speaking (acceptance tests 11 and 21) ---------------------------------------------

describe('Settings > Speaking', () => {
  it('the Speaking tab follows Audio and shows the cost line of speaking_status', async () => {
    await openSpeakingSettings()
    await expectText(COST_LINE)
    expect(callsOf('speaking_status').length).toBeGreaterThan(0)
    const audio = screen.getByRole('button', { name: 'Audio' })
    const speaking = screen.getByRole('button', { name: 'Speaking' })
    expect(audio.compareDocumentPosition(speaking) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    await expectText(KEEP_NOTE)
  })

  it('shows each model with Download ({size}) before any download, then the progress and Downloaded (acceptance test 11)', async () => {
    let finish: (models: unknown) => void = () => {}
    reply('whisper_download', () => new Promise((resolve) => (finish = resolve)))
    await openSpeakingSettings()
    for (const size of ['77.7 MB', '148.0 MB', '487.6 MB']) {
      expect(await screen.findByRole('button', { name: `Download (${size})` })).toBeInTheDocument()
    }
    expect(callsOf('whisper_download'), 'the size is shown before any call').toHaveLength(0)

    fireEvent.click(screen.getByRole('button', { name: 'Download (148.0 MB)' }))
    await waitFor(() => expect(callsOf('whisper_download')).toEqual([{ name: 'base' }]))
    await listening(PROGRESS)
    // 74,000,000 of 147,951,465 bytes is 50.02%.
    emitEvent(PROGRESS, { name: 'base', bytes_done: 74_000_000, bytes_total: 147_951_465 })
    await expectText('Downloading base: 50%')
    await act(async () => {
      finish(MODELS.map((model) => (model.name === 'base' ? { ...model, downloaded: true } : model)))
    })
    await expectText('Downloaded')
    expect(screen.queryByRole('button', { name: 'Download (148.0 MB)' })).toBeNull()
  })

  it('offers AI provider and Local model, and Use {name} saves speaking.whisper_model', async () => {
    await openSpeakingSettings()
    expect(await findControl('AI provider')).toBeInTheDocument()
    expect(await findControl('Local model')).toBeInTheDocument()
    fireEvent.click(await findControl('Use tiny'))
    await waitFor(() =>
      expect(callsOf('settings_set')).toContainEqual({ patch: { speaking: { whisper_model: 'tiny' } } }),
    )
  })

  it('labels every control and saves each change under speaking.*', async () => {
    await openSpeakingSettings()
    await expectText('Local speech models')
    await expectText('Check my speaking with')
    await expectText('base, 148.0 MB')
    await expectText('Pass mark')
    await expectText('80%')

    const saved = (patch: Record<string, unknown>) =>
      waitFor(() => expect(callsOf('settings_set')).toContainEqual({ patch: { speaking: patch } }))

    fireEvent.click(screen.getByRole('switch', { name: 'Speaking exercises' }))
    await saved({ enabled: false })
    fireEvent.click(await findControl('Local model'))
    await saved({ transcriber: 'local' })
    fireEvent.click(screen.getByRole('switch', { name: 'Keep my recordings' }))
    await saved({ keep_recordings: true })
    // The pass mark is saved in whole steps of 0.05: 0.87 is 7.4 steps above 0.5, so 0.85.
    fireEvent.change(screen.getByRole('slider', { name: 'Pass mark' }), { target: { value: '0.87' } })
    await saved({ pass_threshold: 0.85 })
  })

  it('without local Whisper in the build: the line, and no Local model option or model list (acceptance test 21)', async () => {
    reply('speaking_status', () => ({ ...SPEAKING_STATUS, local_available: false }))
    await openSpeakingSettings()
    await expectText(NOT_IN_BUILD)
    expect(await findControl('AI provider')).toBeInTheDocument()
    await settle()
    expect(controls('Local model')).toHaveLength(0)
    expect(screen.queryAllByRole('button', { name: /^Download \(/ })).toHaveLength(0)
    expect(controls(/^Use (tiny|base|small)$/)).toHaveLength(0)
  })
})
