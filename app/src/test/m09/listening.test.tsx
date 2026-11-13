// Module 09 UI acceptance tests (docs/modules/09-listening-and-voices.md, UI; acceptance test 16):
// autoplay of a target-language prompt and of the correct answer, the prefetch of each exercise
// shown, Play and Slow with their notice, errors and Stop, Can't listen now in a listen-and-type
// exercise, and Settings > Audio. The app is rendered whole; the Tauri bridge answers from a table
// of the spec's result shapes (TtsPlayed, Prefetched, AudioStatus, Voice, CacheCleared, AppError)
// and records every call with the snake_case arguments the spec gives; every element is found by
// its role and the visible text or label the spec names.
//
// Where the spec leaves a detail open, these tests assume that Settings > Audio is opened by a tab
// button named Audio, as the other parts of Settings are, and that System voice and Natural voice
// are buttons or radio buttons.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
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

const NATURAL_UNAVAILABLE = 'Natural voice unavailable, using the system voice.'
const NO_SPANISH_VOICE =
  'No Spanish voice is installed. On Windows, open Settings > Time & language > Speech and add a voice.'
const COST_LINE = 'Natural voices cost about $0.00054 a sentence (measured).'
const BALANCE = 'Your publik balance is $0.00. Add a plan or pack to keep using AI features.'
const OFFLINE = "You're offline. AI features need a connection or a local model."
const ENDED = 'audio://ended'
/** Can't listen now, with either apostrophe. */
const CANT_LISTEN = /can.t listen now/i

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

/** The settings with some audio and AI settings changed. */
function settingsWith(audio: Record<string, unknown>, ai: Record<string, unknown> = {}) {
  return { ...SETTINGS, audio: { ...SETTINGS.audio, ...audio }, ai: { ...SETTINGS.ai, ...ai } }
}

const COURSES = [
  { lang: 'es', name: 'Spanish', started: true, active: true, placement_done: true, cefr_estimate: null, skills: 8 },
]

const PATH = {
  lang: 'es',
  units: [
    {
      id: 'es.a1.u03',
      n: 3,
      title: 'Food and drink',
      cefr: 'A1',
      skills: [
        {
          id: 'es.a1.u03.s01',
          title: 'Food and drink',
          can_do: 'I can order simple food and drinks.',
          cefr: 'A1',
          state: 'available',
          mastery: 0,
          tested_out: false,
        },
      ],
      story: null,
    },
  ],
  next_skill_id: 'es.a1.u03.s01',
  due: 0,
  streak: 4,
  freezes: 1,
  today_xp: 12,
  goal_xp: 20,
}

const SESSION = 's-0000000000000009'

/** A pick_meaning exercise: its prompt is in Spanish. */
const PICK = {
  id: 'e1',
  item_id: 'es.a1.u03.s01#l:café',
  facet: 'receptive',
  origin: 'template',
  type: 'pick_meaning',
  prompt_target: 'el café',
  options: ['coffee', 'tea', 'water', 'milk'],
  answer_index: 0,
}

const PICK_TEA = {
  id: 'e2',
  item_id: 'es.a1.u03.s01#l:té',
  facet: 'receptive',
  origin: 'template',
  type: 'pick_meaning',
  prompt_target: 'el té',
  options: ['tea', 'coffee', 'water', 'milk'],
  answer_index: 0,
}

/** A translation into Spanish: its prompt is English and its correct answer Spanish. */
const FIRST = 'Quiero un café con leche.'
const COFFEE = {
  id: 'e1',
  item_id: 'es.a1.u03.s01#e:2',
  facet: 'productive',
  origin: 'template',
  type: 'translate_to_target',
  prompt_en: 'I want a coffee with milk.',
  target: FIRST,
  accepted: [FIRST],
}

const LISTEN = {
  id: 'e1',
  item_id: 'es.a1.u03.s01#e:2',
  facet: 'receptive',
  origin: 'template',
  type: 'listen_type',
  text: FIRST,
  accepted: [FIRST],
}

const LISTEN_TOO = {
  id: 'e2',
  item_id: 'es.a1.u03.s01#e:1',
  facet: 'receptive',
  origin: 'template',
  type: 'listen_type',
  text: 'Un café, por favor.',
  accepted: ['Un café, por favor.'],
}

const PICK_LAST = { ...PICK, id: 'e3' }

function lesson(exercises: object[]) {
  return {
    session_id: SESSION,
    kind: 'lesson',
    skill_id: 'es.a1.u03.s01',
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

/** TtsPlayed. */
function played(extra: Record<string, unknown> = {}) {
  return { source: 'os', cached: false, notice: null, ai_error: null, ...extra }
}

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

function answerDefaults() {
  fake.answers.clear()
  fake.calls.length = 0
  fake.listeners.clear()
  aiStatus = STATUS_PUBLIK
  reply('app_start', () => ({ onboarded: true, active_course: 'es', notice: null }))
  reply('app_info', () => ({ version: '9.9.9', data_dir: 'parley-data-folder' }))
  reply('settings_get', () => SETTINGS)
  // The command returns the whole settings with the patch merged in, nested objects included.
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
  reply('lesson_start', () => lesson([PICK]))
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
  reply('voices_list', (args) =>
    args.lang === 'es' ? [{ id: 'os-es-ES', name: 'Spanish (Spain)', source: 'os', installed: true }] : [],
  )
  reply('tts_play', () => played())
  reply('tts_stop', () => null)
  reply('tts_prefetch', () => ({ prefetched: false }))
  reply('audio_status', () => ({ cost_line: COST_LINE, listen_off_until: null }))
  reply('session_skip', () => null)
  reply('cache_clear', () => ({ files_removed: 3 }))
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

/** A button or radio button named `name`. */
async function findControl(name: string): Promise<HTMLElement> {
  return waitFor(() => {
    const found = [...screen.queryAllByRole('button', { name }), ...screen.queryAllByRole('radio', { name })]
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

async function openAudioSettings() {
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
  await screen.findByRole('heading', { name: 'Settings' })
  fireEvent.click(await screen.findByRole('button', { name: 'Audio' }))
}

/** The `audio.voice_source` of every settings_set patch, in order. */
function savedVoiceSources(): unknown[] {
  return callsOf('settings_set').map((args) => {
    const patch = args.patch as { audio?: { voice_source?: unknown } } | undefined
    return patch?.audio?.voice_source
  })
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  cleanup()
})

// ---- acceptance test 16: autoplay -------------------------------------------------------------

describe('Autoplay (acceptance test 16)', () => {
  it('with autoplay on, showing a pick_meaning exercise calls tts_play with its prompt once', async () => {
    await startLesson()
    await waitFor(() => expect(callsOf('tts_play')).toHaveLength(1))
    expect(callsOf('tts_play')[0]).toEqual({ text: 'el café', lang: 'es', slow: false, session_id: SESSION })
    await settle()
    expect(callsOf('tts_play')).toHaveLength(1)
  })

  it('with autoplay off, showing a pick_meaning exercise does not call tts_play', async () => {
    reply('settings_get', () => settingsWith({ autoplay: false }))
    await startLesson()
    await expectText('el café')
    await waitFor(() => expect(callsOf('settings_get').length).toBeGreaterThan(0))
    await settle()
    expect(callsOf('tts_play')).toHaveLength(0)
  })

  it('with autoplay on, the result sheet reads the correct answer of a translation into Spanish once', async () => {
    reply('lesson_start', () => lesson([COFFEE]))
    await startLesson()
    await expectText('Translate into Spanish')
    await settle()
    expect(callsOf('tts_play'), 'the English prompt is not read').toHaveLength(0)
    fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: FIRST } })
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    await screen.findByRole('region', { name: 'Result' })
    await waitFor(() => expect(callsOf('tts_play')).toHaveLength(1))
    expect(callsOf('tts_play')[0]).toEqual({ text: FIRST, lang: 'es', slow: false, session_id: SESSION })
    await settle()
    expect(callsOf('tts_play')).toHaveLength(1)
  })
})

// ---- prefetch -----------------------------------------------------------------------------------

describe('Prefetch (UI)', () => {
  it('calls tts_prefetch for each exercise it shows and ignores its errors', async () => {
    reply('settings_get', () => settingsWith({ autoplay: false }))
    reply('lesson_start', () => lesson([PICK, PICK_TEA]))
    reply('tts_prefetch', () => Promise.reject(appError('offline', OFFLINE)))
    await startLesson()
    await waitFor(() =>
      expect(callsOf('tts_prefetch')).toContainEqual({ session_id: SESSION, exercise_id: 'e1' }),
    )
    await settle()
    expect(hasText(OFFLINE), 'a failed prefetch is not shown').toBe(false)

    fireEvent.click(await screen.findByRole('radio', { name: 'coffee' }))
    fireEvent.click(screen.getByRole('button', { name: 'Check' }))
    const result = await screen.findByRole('region', { name: 'Result' })
    fireEvent.click(within(result).getByRole('button', { name: 'Continue' }))
    await expectText('el té')
    await waitFor(() =>
      expect(callsOf('tts_prefetch')).toContainEqual({ session_id: SESSION, exercise_id: 'e2' }),
    )
  })
})

// ---- PlayButton ---------------------------------------------------------------------------------

describe('Play and Slow (UI)', () => {
  beforeEach(() => {
    reply('settings_get', () => settingsWith({ autoplay: false }))
    reply('lesson_start', () => lesson([LISTEN]))
  })

  it('Play and Slow call tts_play, and the notice of a fallback shows under them', async () => {
    reply('tts_play', () => played({ notice: NATURAL_UNAVAILABLE }))
    await startLesson()
    fireEvent.click(await screen.findByRole('button', { name: 'Play the sentence' }))
    await waitFor(() =>
      expect(callsOf('tts_play')).toEqual([{ text: FIRST, lang: 'es', slow: false, session_id: SESSION }]),
    )
    await expectText(NATURAL_UNAVAILABLE)

    emitEvent(ENDED, null)
    fireEvent.click(await screen.findByRole('button', { name: 'Play it slowly' }))
    await waitFor(() => expect(callsOf('tts_play')).toHaveLength(2))
    expect(callsOf('tts_play')[1]).toEqual({ text: FIRST, lang: 'es', slow: true, session_id: SESSION })
  })

  it('shows Stop while the sentence plays: audio://ended brings Play back, and Stop calls tts_stop', async () => {
    await startLesson()
    fireEvent.click(await screen.findByRole('button', { name: 'Play the sentence' }))
    await screen.findByRole('button', { name: 'Stop' })
    emitEvent(ENDED, null)
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Stop' })).toBeNull())
    expect(screen.getByRole('button', { name: 'Play the sentence' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Play the sentence' }))
    const stop = await screen.findByRole('button', { name: 'Stop' })
    const stopsBefore = callsOf('tts_stop').length
    fireEvent.click(stop)
    await waitFor(() => expect(callsOf('tts_stop')).toHaveLength(stopsBefore + 1))
    expect(callsOf('tts_stop').at(-1)).toEqual({})
  })

  it('shows the ai_error of a play as the AI banner shows it', async () => {
    reply('tts_play', () =>
      played({ ai_error: appError('needs_credit', BALANCE, { top_up_url: 'https://publikhq.com/wallet/add-credit?from=402' }) }),
    )
    await startLesson()
    fireEvent.click(await screen.findByRole('button', { name: 'Play the sentence' }))
    await expectText(BALANCE)
  })

  it("shows a no_voice error's message", async () => {
    reply('tts_play', () => Promise.reject(appError('no_voice', NO_SPANISH_VOICE)))
    await startLesson()
    fireEvent.click(await screen.findByRole('button', { name: 'Play the sentence' }))
    await expectText(NO_SPANISH_VOICE)
  })
})

// ---- ListenType ---------------------------------------------------------------------------------

describe("Can't listen now (UI)", () => {
  it('calls session_skip with listen and skips this and every later listen_type exercise with no grade', async () => {
    reply('settings_get', () => settingsWith({ autoplay: false }))
    reply('lesson_start', () => lesson([LISTEN, LISTEN_TOO, PICK_LAST]))
    await startLesson()
    expect(await screen.findByRole('button', { name: 'Play the sentence' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Play it slowly' })).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Your answer' })).toBeInTheDocument()

    fireEvent.click(await screen.findByRole('button', { name: CANT_LISTEN }))
    await waitFor(() => expect(callsOf('session_skip')).toEqual([{ kind: 'listen' }]))
    await expectText('el café')
    expect(screen.queryByRole('button', { name: CANT_LISTEN })).toBeNull()
    expect(callsOf('exercise_submit'), 'skipped with no grade').toHaveLength(0)
    expect(callsOf('session_skip')).toHaveLength(1)
  })
})

// ---- Settings > Audio ---------------------------------------------------------------------------

describe('Settings > Audio (UI)', () => {
  it('shows the measured cost line from audio_status', async () => {
    await openAudioSettings()
    await expectText(COST_LINE)
    expect(callsOf('audio_status').length).toBeGreaterThan(0)
  })

  it('Clear audio cache calls cache_clear with audio and says how many files it removed', async () => {
    await openAudioSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Clear audio cache' }))
    await waitFor(() => expect(callsOf('cache_clear')).toEqual([{ kind: 'audio' }]))
    await expectText('Removed 3 files.')
  })

  it('shows the OS voices of voices_list', async () => {
    await openAudioSettings()
    await waitFor(() => expect(callsOf('voices_list')).toContainEqual({ lang: 'es' }))
    await expectText('Spanish (Spain)')
  })

  it('says No voice installed for a language with no OS voice', async () => {
    reply('voices_list', () => [])
    await openAudioSettings()
    await expectText('No voice installed')
  })

  it('Natural voice with no provider chosen opens the AI setup dialog, and Not now keeps the system voice (I12)', async () => {
    aiStatus = STATUS_UNSET
    reply('settings_get', () => settingsWith({}, { provider: 'unset' }))
    await openAudioSettings()
    fireEvent.click(await findControl('System voice'))
    fireEvent.click(await findControl('Natural voice'))
    const dialog = await screen.findByRole('dialog', { name: 'Set up AI features' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Not now' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await settle()
    expect(savedVoiceSources()).not.toContain('natural')
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('choosing a system voice saves it for that language', async () => {
    reply('voices_list', (args) =>
      args.lang === 'es'
        ? [
            { id: 'os-es-ES', name: 'Spanish (Spain)', source: 'os', installed: true },
            { id: 'os-es-MX', name: 'Spanish (Mexico)', source: 'os', installed: true },
          ]
        : [],
    )
    await openAudioSettings()
    const picker = await screen.findByRole('combobox', { name: 'Spanish voice' })
    fireEvent.change(picker, { target: { value: 'os-es-MX' } })
    await waitFor(() =>
      expect(callsOf('settings_set')).toContainEqual({ patch: { audio: { os_voice: { es: 'os-es-MX' } } } }),
    )
  })

  it('Slow speed saves the chosen speed rounded to a whole step of 0.05', async () => {
    await openAudioSettings()
    const slider = await screen.findByRole('slider', { name: 'Slow speed' })
    fireEvent.change(slider, { target: { value: '0.62' } })
    await waitFor(() =>
      expect(callsOf('settings_set')).toContainEqual({ patch: { audio: { slow_speed: 0.6 } } }),
    )
  })

  it('Play prompts aloud turns autoplay off', async () => {
    await openAudioSettings()
    const toggle = await screen.findByRole('switch', { name: 'Play prompts aloud' })
    fireEvent.click(toggle)
    await waitFor(() =>
      expect(callsOf('settings_set')).toContainEqual({ patch: { audio: { autoplay: false } } }),
    )
  })

  it('Natural voice with publik API set up saves audio.voice_source natural', async () => {
    await openAudioSettings()
    fireEvent.click(await findControl('Natural voice'))
    await waitFor(() => expect(savedVoiceSources()).toContain('natural'))
    expect(screen.queryByRole('dialog', { name: 'Set up AI features' })).toBeNull()
  })
})
