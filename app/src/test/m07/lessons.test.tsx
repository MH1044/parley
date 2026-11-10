// Module 07 UI acceptance tests (docs/modules/07-lesson-generation.md, UI; acceptance tests 15, 22
// and 23): the app is rendered whole, the Tauri bridge answers from a table of the spec's result
// shapes, and every element is found by its role and the visible text the spec gives.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

type Args = Record<string, unknown>

const fake = vi.hoisted(() => ({
  answers: new Map<string, (args: Record<string, unknown>) => unknown>(),
  calls: [] as { command: string; args: Record<string, unknown> }[],
  listeners: new Map<string, ((event: { event: string; id: number; payload: unknown }) => void)[]>(),
}))

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

const WRITING = 'Writing a fresh lesson…'
const USE_PACK = 'Use the course pack instead'
const WRITING_STORY = 'Writing a story…'
const INVALID = 'The AI answer could not be used.'
const NOT_YET = 'Practise the last skill of this unit first.'
const IMPORTED =
  "Imported 2 lessons and 1 stories; 3 exercises were dropped because they did not pass Parley's checks."
const NOT_A_PACK = 'This is not a Parley course pack.'
const LOCAL_RATE = "Local model: 75% of AI exercises passed Parley's checks (last 3 lessons)."

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
}

const STATUS_LOCAL = { ...STATUS_UNSET, provider: 'local', needs_consent: false, local_model: 'llama3.1:8b' }

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

const COURSES = [
  { lang: 'es', name: 'Spanish', started: true, active: true, placement_done: true, cefr_estimate: null, skills: 8 },
]

function pathWith(story: { skill_id: string; available: boolean } | null) {
  return {
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
        story,
      },
    ],
    next_skill_id: 'es.a1.u01.s01',
    due: 0,
    streak: 4,
    freezes: 1,
    today_xp: 12,
    goal_xp: 20,
  }
}

const PICK = {
  id: 'e1',
  item_id: 'es.a1.u01.s01#l:hola',
  facet: 'receptive',
  origin: 'template',
  type: 'pick_meaning',
  prompt_target: 'hola',
  options: ['hello', 'goodbye', 'thank you', 'good night'],
  answer_index: 0,
}

const TEMPLATE_LESSON = {
  session_id: 's-0000000000000001',
  kind: 'lesson',
  skill_id: 'es.a1.u01.s01',
  origin: 'template',
  exercises: [PICK],
  attributions: [],
  notice: null,
  ai_error: null,
  story: null,
}

const STORY = {
  title: 'Un café en Madrid',
  title_en: 'A coffee in Madrid',
  paragraphs: ['Ana está en un café en Madrid.', 'Luis quiere un té sin azúcar.', 'Ana tiene agua y pan.'],
  questions: [
    { question_en: 'Where is Ana?', options: ['In a café', 'At home', 'At school'], answer_index: 0, paragraph: 0 },
    { question_en: 'What does Luis want?', options: ['Tea', 'Milk', 'Juice'], answer_index: 0, paragraph: 1 },
    { question_en: 'What does Ana have?', options: ['Water and bread', 'Coffee', 'Nothing'], answer_index: 0, paragraph: 2 },
  ],
}

const STORY_STARTED = {
  session_id: 's-0000000000000002',
  kind: 'story',
  skill_id: 'es.a1.u01.s01',
  origin: 'ai',
  exercises: [],
  attributions: [],
  notice: null,
  ai_error: null,
  story: STORY,
}

function grade(verdict: string, corrected: string | null = null) {
  return { verdict, tags: [], message: null, corrected, words: [], rating: null }
}

// ---- the fake bridge ------------------------------------------------------------------------------

function reply(command: string, answer: (args: Args) => unknown) {
  fake.answers.set(command, answer)
}

function callsOf(command: string): Args[] {
  return fake.calls.filter((call) => call.command === command).map((call) => call.args)
}

let aiStatus: Record<string, unknown> = STATUS_PUBLIK

function answerDefaults() {
  fake.answers.clear()
  fake.calls.length = 0
  fake.listeners.clear()
  aiStatus = STATUS_PUBLIK
  reply('app_start', () => ({ onboarded: true, active_course: 'es', notice: null }))
  reply('app_info', () => ({ version: '9.9.9', data_dir: 'parley-data-folder' }))
  reply('settings_get', () => SETTINGS)
  reply('settings_set', (args) => ({ ...SETTINGS, ...(args.patch as Record<string, unknown>) }))
  reply('course_list', () => COURSES)
  reply('pack_status', () => ({ lang: 'es', state: 'installed', version: '2026-10-01', bytes: 8_200_000, size: '8.2 MB' }))
  reply('path_get', () => pathWith(null))
  reply('stats_get', () => ({ lang: 'es', empty: true, streak: 0, freezes: 0, days: [] }))
  reply('ai_status', () => aiStatus)
  reply('ai_lesson_checks', () => ({ lessons: 0, valid: 0, planned: 0, message: null }))
}

// ---- finding things -------------------------------------------------------------------------------

const squash = (text: string | null | undefined) => (text ?? '').replace(/\s+/g, ' ').trim()

function textOf(text: string) {
  return (_content: string, element: Element | null): boolean =>
    element !== null &&
    squash(element.textContent).includes(text) &&
    !Array.from(element.children).some((child) => squash(child.textContent).includes(text))
}

async function expectText(text: string, container: HTMLElement = document.body) {
  await waitFor(() => expect(within(container).queryAllByText(textOf(text)).length).toBeGreaterThan(0))
}

function hasText(text: string) {
  return screen.queryAllByText(textOf(text)).length > 0
}

/** A switch or checkbox named `name` (the spec says switch; either role is accepted). */
async function toggle(name: string): Promise<HTMLElement> {
  return waitFor(() => {
    const found = [...screen.queryAllByRole('switch', { name }), ...screen.queryAllByRole('checkbox', { name })]
    expect(found.length, `a switch named ${name}`).toBeGreaterThan(0)
    return found[0]
  })
}

async function openPath() {
  render(<App />)
  await expectText('4 day streak')
}

async function openSettingsTab(tab: string) {
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
  await screen.findByRole('heading', { name: 'Settings' })
  fireEvent.click(await screen.findByRole('button', { name: tab }))
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  vi.useRealTimers()
  cleanup()
})

// ---- acceptance test 15: Writing a fresh lesson, and the course pack after 5 s ------------------

describe('Loading a fresh lesson (acceptance test 15)', () => {
  it('offers Use the course pack instead after 5 s, not before, and starts that lesson', async () => {
    reply('lesson_start', () => new Promise(() => {}))
    reply('lesson_use_course_pack', () => TEMPLATE_LESSON)
    await openPath()
    const start = await screen.findByRole('button', { name: 'Continue' })

    vi.useFakeTimers()
    await act(async () => {
      fireEvent.click(start)
      await vi.advanceTimersByTimeAsync(0)
    })
    expect(callsOf('lesson_start')).toContainEqual({ skill_id: 'es.a1.u01.s01', mode: 'learn' })
    const status = screen.getByRole('status')
    expect(squash(status.textContent)).toContain(WRITING)
    expect(screen.queryByRole('button', { name: USE_PACK })).toBeNull()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(4_900)
    })
    expect(screen.queryByRole('button', { name: USE_PACK })).toBeNull()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(200)
    })
    const instead = screen.getByRole('button', { name: USE_PACK })
    vi.useRealTimers()
    fireEvent.click(instead)
    await waitFor(() => expect(callsOf('lesson_use_course_pack')).toEqual([{ skill_id: 'es.a1.u01.s01' }]))
    await expectText('hola')
  })

  it('shows the notice above the first exercise', async () => {
    reply('lesson_start', () => ({
      ...TEMPLATE_LESSON,
      notice: 'A fresh lesson was not available, so this one comes from the course pack.',
    }))
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
    await expectText('A fresh lesson was not available, so this one comes from the course pack.')
    await expectText('hola')
  })

  it('shows a 402 as the AI banner with its one link', async () => {
    reply('lesson_start', () => ({
      ...TEMPLATE_LESSON,
      ai_error: {
        code: 'needs_credit',
        message: 'Your publik balance is $0.00.',
        action: { top_up_url: 'https://publikhq.com/wallet/top-up' },
      },
    }))
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
    await waitFor(() => {
      const alert = screen.queryAllByRole('alert').find((a) => squash(a.textContent).includes('Your publik balance is $0.00.'))
      expect(alert).toBeDefined()
    })
    await expectText('hola')
  })
})

// ---- acceptance test 22 and 23: the Story node and the Story screen ------------------------------

describe('Stories (acceptance tests 22 and 23)', () => {
  it('shows no Story node when the path has none', async () => {
    await openPath()
    expect(screen.queryByRole('button', { name: 'Read a story' })).toBeNull()
  })

  it('disables the Story node until the last skill is practised', async () => {
    reply('path_get', () => pathWith({ skill_id: 'es.a1.u01.s01', available: false }))
    await openPath()
    const node = await screen.findByRole('button', { name: 'Read a story' })
    expect(node).toBeDisabled()
    await expectText(NOT_YET)
  })

  it('reads a story, grades each question and finishes with the XP', async () => {
    reply('path_get', () => pathWith({ skill_id: 'es.a1.u01.s01', available: true }))
    reply('lesson_start', () => STORY_STARTED)
    reply('exercise_submit', (args) => {
      const answer = args.answer as { choice: number }
      return answer.choice === 0 ? grade('correct') : grade('wrong', 'Tea')
    })
    reply('lesson_finish', () => ({
      xp: 14,
      correct: 2,
      total: 3,
      streak: 5,
      freezes: 1,
      record_week: false,
      message: null,
      daily_xp: 26,
      goal_xp: 20,
    }))
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Read a story' }))
    await waitFor(() => expect(callsOf('lesson_start')).toContainEqual({ skill_id: 'es.a1.u01.s01', mode: 'story' }))
    await expectText('Un café en Madrid')
    await expectText('A coffee in Madrid')
    await expectText('Ana está en un café en Madrid.')
    await expectText('Where is Ana?')

    fireEvent.click(screen.getByRole('button', { name: 'In a café' }))
    await waitFor(() => expect(callsOf('exercise_submit')[0]).toMatchObject({ exercise_id: 'q1', answer: { choice: 0 } }))
    await expectText('Correct')

    fireEvent.click(await screen.findByRole('button', { name: 'Juice' }))
    await waitFor(() => expect(callsOf('exercise_submit')[1]).toMatchObject({ exercise_id: 'q2', answer: { choice: 2 } }))
    await expectText('Wrong: Tea')

    fireEvent.click(await screen.findByRole('button', { name: 'Water and bread' }))
    await waitFor(() => expect(callsOf('exercise_submit')).toHaveLength(3))
    fireEvent.click(await screen.findByRole('button', { name: 'Finish' }))
    await waitFor(() => expect(callsOf('lesson_finish')).toEqual([{ session_id: 's-0000000000000002' }]))
    await expectText('14')
  })

  it('offers Try again when the AI answer could not be used', async () => {
    reply('path_get', () => pathWith({ skill_id: 'es.a1.u01.s01', available: true }))
    reply('lesson_start', () => Promise.reject({ code: 'ai_invalid', message: INVALID, action: null }))
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Read a story' }))
    await expectText(INVALID)
    reply('lesson_start', () => STORY_STARTED)
    fireEvent.click(await screen.findByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(callsOf('lesson_start')).toHaveLength(2))
    expect(callsOf('lesson_start')[1]).toEqual({ skill_id: 'es.a1.u01.s01', mode: 'story' })
    await expectText('Un café en Madrid')
  })

  it('says it is writing while the story is pending', async () => {
    reply('path_get', () => pathWith({ skill_id: 'es.a1.u01.s01', available: true }))
    reply('lesson_start', () => new Promise(() => {}))
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Read a story' }))
    await expectText(WRITING_STORY)
  })

  it('opens the consent dialog when no provider is set up', async () => {
    aiStatus = STATUS_UNSET
    reply('path_get', () => pathWith({ skill_id: 'es.a1.u01.s01', available: true }))
    reply('lesson_start', () =>
      Promise.reject({ code: 'needs_consent', message: 'AI features are not set up yet.', action: null }),
    )
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Read a story' }))
    expect(await screen.findByRole('dialog', { name: 'Set up AI features' })).toBeInTheDocument()
  })
})

// ---- acceptance test 23: Settings > AI ------------------------------------------------------------

describe('Settings > AI switches (acceptance test 23)', () => {
  it('turns the three AI lesson switches with settings_set', async () => {
    await openSettingsTab('AI')
    for (const [name, key] of [
      ['Fresh lessons', 'fresh_lessons'],
      ['Prepare the next lesson', 'prepare_next_lesson'],
      ['Stories', 'stories'],
    ] as const) {
      const control = await toggle(name)
      expect(control).toBeChecked()
      fireEvent.click(control)
      await waitFor(() =>
        expect(callsOf('settings_set').map((args) => args.patch)).toContainEqual({ ai: { [key]: false } }),
      )
    }
  })

  it('shows the local model validity rate', async () => {
    aiStatus = STATUS_LOCAL
    reply('ai_lesson_checks', () => ({ lessons: 3, valid: 27, planned: 36, message: LOCAL_RATE }))
    await openSettingsTab('AI')
    await expectText(LOCAL_RATE)
  })
})

// ---- acceptance test 23: Settings > Data, course packs -------------------------------------------

describe('Course packs in Settings > Data (acceptance test 23)', () => {
  it('imports a course pack and shows the report', async () => {
    reply('file_pick', () => 'pack.json')
    reply('course_pack_import', () => ({ lessons: 2, stories: 1, dropped: 3, message: IMPORTED }))
    await openSettingsTab('Data')
    fireEvent.click(await screen.findByRole('button', { name: 'Import a course pack' }))
    await waitFor(() => expect(callsOf('file_pick')).toContainEqual({ kind: 'course_pack_import' }))
    await waitFor(() => expect(callsOf('course_pack_import')).toEqual([{ src_path: 'pack.json' }]))
    await expectText(IMPORTED)
  })

  it('shows why a file was refused', async () => {
    reply('file_pick', () => 'notes.json')
    reply('course_pack_import', () => Promise.reject({ code: 'invalid_input', message: NOT_A_PACK, action: null }))
    await openSettingsTab('Data')
    fireEvent.click(await screen.findByRole('button', { name: 'Import a course pack' }))
    await expectText(NOT_A_PACK)
  })

  it('exports the active course', async () => {
    reply('file_pick', () => 'parley-course-es.json')
    reply('course_pack_export', () => ({ bytes: 2_048, lessons: 2, stories: 1, exercises: 20 }))
    await openSettingsTab('Data')
    fireEvent.click(await screen.findByRole('button', { name: 'Export a course pack' }))
    await waitFor(() => expect(callsOf('file_pick')).toContainEqual({ kind: 'course_pack_export' }))
    await waitFor(() =>
      expect(callsOf('course_pack_export')).toEqual([{ lang: 'es', dest_path: 'parley-course-es.json' }]),
    )
    await expectText('Exported 2 lessons and 1 stories.')
  })

  it('does nothing when the picker is cancelled', async () => {
    reply('file_pick', () => null)
    await openSettingsTab('Data')
    fireEvent.click(await screen.findByRole('button', { name: 'Import a course pack' }))
    await waitFor(() => expect(callsOf('file_pick')).toHaveLength(1))
    expect(callsOf('course_pack_import')).toHaveLength(0)
    expect(hasText(IMPORTED)).toBe(false)
  })
})
