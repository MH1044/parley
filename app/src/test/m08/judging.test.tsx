// Module 08 UI acceptance tests (docs/modules/08-judging-and-explanations.md, UI; acceptance tests
// 6, 8 to 12): the app is rendered whole, the Tauri bridge answers from a table of the spec's
// result shapes (Grade with its optional explanation, expected and judge_error; the result of
// explain_answer; AppError), and every element is found by its role and the visible text the spec
// and docs/PUBLIK-INTEGRATION.md §9 give.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

type Args = Record<string, unknown>

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

const CHECKING = 'Checking…'
const NOT_CHECKED = 'Not checked'
const CHECK_WITH_AI = 'Check with AI'
const I_WAS_RIGHT = 'I was right'
const EXPLAIN = 'Explain'
const EXPLAINING = 'Explaining…'
const TRY_AGAIN = 'Try again'
const JUDGE_OFFLINE = "You're offline, so Parley can't check free answers."
const EXPLAIN_OFFLINE = 'Explanations need a connection or a local model.'
const TIMED_OUT = 'publik API did not answer in time.'
const INVALID = 'The AI answer could not be used.'
const BUSY = 'publik API is busy; try again in a moment'
const DISCONNECTED = 'publik API is disconnected.'
const NOT_SET_UP = 'AI features are not set up yet.'
const BALANCE = 'Your publik balance is $0.00. Add a plan or pack to keep using AI features.'
const OR_ELSE = 'or use your own key or a local model in Settings'
const LINKS = ['Sign in with publik', 'Add a plan or pack']

/** The §6.5 example: the exercise, the answer, the verdict's corrected answer and reason. */
const ANSWER = 'Me gustaría una café con leche'
const CORRECTED = 'Me gustaría un café con leche'
const FIRST = 'Quiero un café con leche.'
const ALMOST_WHY = 'Café is masculine, so it takes un, not una.'
const WRONG_WHY = 'Té is tea, not coffee, and me gusta means I like; to say I would like, use me gustaría.'

/** The §6.6 example response. */
const EXPLAINED = {
  text: 'You wrote "una café". Café is a masculine noun, so it needs the masculine article: un café. "Me gustaría" is a polite, correct way to say "I would like".',
  tip: 'Learn each noun with its article: un café.',
  cached: false,
}

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

const SESSION = 's-0000000000000008'

/** The §6.5 exercise. */
const COFFEE = {
  id: 'e1',
  item_id: 'es.a1.u03.s01#e:2',
  facet: 'productive',
  origin: 'course_pack',
  type: 'translate_to_target',
  prompt_en: 'I would like a coffee with milk.',
  target: FIRST,
  accepted: [FIRST, 'Quisiera un café con leche.'],
}

const PAIRS = {
  id: 'e1',
  item_id: 'es.a1.u03.s01#l:té',
  facet: 'receptive',
  origin: 'course_pack',
  type: 'match_pairs',
  pairs: [
    ['el té', 'tea'],
    ['el café', 'coffee'],
    ['el agua', 'water'],
    ['el pan', 'bread'],
  ],
}

function lesson(exercises: object[]) {
  return {
    session_id: SESSION,
    kind: 'lesson',
    skill_id: 'es.a1.u03.s01',
    origin: 'course_pack',
    exercises,
    attributions: [],
    notice: null,
    ai_error: null,
    story: null,
  }
}

type GradeExtra = Partial<{
  tags: string[]
  message: string | null
  corrected: string | null
  words: string[]
  rating: number | null
  explanation: string
  expected: string
  judge_error: { code: string; message: string; action: Record<string, unknown> | null }
}>

function grade(verdict: string, extra: GradeExtra = {}) {
  return { verdict, tags: [], message: null, corrected: null, words: [], rating: null, ...extra }
}

const ALMOST = grade('almost', { corrected: CORRECTED, rating: 2, explanation: ALMOST_WHY })
const WRONG = grade('wrong', { corrected: CORRECTED, rating: 1, explanation: WRONG_WHY, expected: FIRST })
const UNCHECKED = grade('unchecked')

function appError(code: string, message: string, action: Record<string, unknown> | null = null) {
  return { code, message, action }
}

function notChecked(code: string, message: string, action: Record<string, unknown> | null = null) {
  return grade('unchecked', { judge_error: appError(code, message, action) })
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
  reply('path_get', () => PATH)
  reply('stats_get', () => ({ lang: 'es', empty: true, streak: 0, freezes: 0, days: [] }))
  reply('ai_status', () => aiStatus)
  reply('ai_lesson_checks', () => ({ lessons: 0, valid: 0, planned: 0, message: null }))
  reply('ai_consent_decline', () => aiStatus)
  reply('ai_open_link', () => null)
  reply('ai_reconnect_publik', () => aiStatus)
  reply('lesson_start', () => lesson([COFFEE]))
  reply('exercise_submit', () => UNCHECKED)
  reply('exercise_i_was_right', () => grade('correct', { rating: 3 }))
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

/** The buttons and links inside `container` named one of `names`. */
function controlsNamed(container: HTMLElement, names: string[]) {
  return names.flatMap((name) => [
    ...within(container).queryAllByRole('button', { name }),
    ...within(container).queryAllByRole('link', { name }),
  ])
}

/** Starts the lesson, types `text` into the translation and checks it; the result sheet. */
async function answerCoffee(text = ANSWER): Promise<HTMLElement> {
  render(<App />)
  await expectText('4 day streak')
  fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
  await expectText('Translate into Spanish')
  fireEvent.change(screen.getByRole('textbox', { name: 'Your answer' }), { target: { value: text } })
  fireEvent.click(screen.getByRole('button', { name: 'Check' }))
  await waitFor(() => expect(callsOf('exercise_submit')).toHaveLength(1))
  expect(callsOf('exercise_submit')[0]).toMatchObject({ session_id: SESSION, exercise_id: 'e1', answer: { text } })
  return screen.findByRole('region', { name: 'Result' })
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  cleanup()
})

// ---- Checking… --------------------------------------------------------------------------------

describe('Checking a free translation (UI)', () => {
  it('shows Checking… and no buttons while exercise_submit of a translation is pending', async () => {
    reply('exercise_submit', () => new Promise(() => {}))
    const result = await answerCoffee()
    await expectText(CHECKING, result)
    expect(within(result).queryAllByRole('button')).toHaveLength(0)
  })

  it('shows Checking… while Check with AI is pending', async () => {
    reply('exercise_check', () => new Promise(() => {}))
    const result = await answerCoffee()
    fireEvent.click(await within(result).findByRole('button', { name: CHECK_WITH_AI }))
    await waitFor(() => expect(callsOf('exercise_check')).toHaveLength(1))
    await expectText(CHECKING)
  })
})

// ---- acceptance test 6: the verdicts ----------------------------------------------------------

describe('Judged verdicts (acceptance test 6)', () => {
  it('almost shows the verdict, the reason, the corrected answer, Explain and Continue', async () => {
    reply('exercise_submit', () => ALMOST)
    const result = await answerCoffee()
    await expectText('Almost', result)
    await expectText(ALMOST_WHY, result)
    await expectText(`Correct answer: ${CORRECTED}`, result)
    expect(within(result).getByRole('button', { name: EXPLAIN })).toBeInTheDocument()
    expect(within(result).getByRole('button', { name: 'Continue' })).toBeInTheDocument()
    expect(within(result).queryByRole('button', { name: CHECK_WITH_AI })).toBeNull()
    expect(hasText(NOT_CHECKED, result)).toBe(false)
  })

  it('wrong shows the corrected answer and Also correct with the first accepted answer', async () => {
    reply('exercise_submit', () => WRONG)
    const result = await answerCoffee('Me gusta un té con leche')
    await expectText('Wrong', result)
    await expectText(WRONG_WHY, result)
    await expectText(`Correct answer: ${CORRECTED}`, result)
    await expectText(`Also correct: ${FIRST}`, result)
    expect(within(result).getByRole('button', { name: EXPLAIN })).toBeInTheDocument()
  })

  it('a judged correct answer shows its reason and offers no Explain', async () => {
    const reason = 'Me gustaría is a polite way to say I would like, so this is right.'
    reply('exercise_submit', () => grade('correct', { rating: 3, explanation: reason }))
    const result = await answerCoffee('Me gustaría un café con leche.')
    await expectText('Correct', result)
    await expectText(reason, result)
    expect(within(result).queryByRole('button', { name: EXPLAIN })).toBeNull()
    expect(within(result).getByRole('button', { name: 'Continue' })).toBeInTheDocument()
  })

  it('a typo offers no Explain', async () => {
    reply('exercise_submit', () =>
      grade('typo', { tags: ['typo'], message: 'Typo: leche', corrected: FIRST, words: ['leche'], rating: 2 }),
    )
    const result = await answerCoffee('Quiero un café con lecje')
    await expectText('Typo: leche', result)
    expect(within(result).queryByRole('button', { name: EXPLAIN })).toBeNull()
  })

  it('a wrong match_pairs answer offers no Explain', async () => {
    reply('lesson_start', () => lesson([PAIRS]))
    reply('exercise_submit', () => grade('wrong', { rating: 1 }))
    render(<App />)
    await expectText('4 day streak')
    fireEvent.click(await screen.findByRole('button', { name: 'Continue' }))
    for (const [target, english] of PAIRS.pairs) {
      fireEvent.click(await screen.findByRole('button', { name: target }))
      fireEvent.click(await screen.findByRole('button', { name: english }))
    }
    const check = screen.queryByRole('button', { name: 'Check' })
    if (check) fireEvent.click(check)
    await waitFor(() => expect(callsOf('exercise_submit')).toHaveLength(1))
    const result = await screen.findByRole('region', { name: 'Result' })
    await expectText('Wrong', result)
    expect(within(result).queryByRole('button', { name: EXPLAIN })).toBeNull()
  })
})

// ---- acceptance test 12: Explain ----------------------------------------------------------------

describe('Explain (acceptance test 12)', () => {
  it('calls explain_answer, shows Explaining… and then the text and the tip', async () => {
    reply('exercise_submit', () => ALMOST)
    let release: (value: unknown) => void = () => {}
    reply(
      'explain_answer',
      () =>
        new Promise((resolve) => {
          release = resolve
        }),
    )
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: EXPLAIN }))
    await waitFor(() => expect(callsOf('explain_answer')).toEqual([{ session_id: SESSION, exercise_id: 'e1' }]))
    await expectText(EXPLAINING)
    await act(async () => {
      release(EXPLAINED)
    })
    await expectText('Café is a masculine noun, so it needs the masculine article: un café.')
    await expectText(`Tip: ${EXPLAINED.tip}`)
    expect(hasText(EXPLAINING)).toBe(false)
    expect(screen.queryByRole('button', { name: EXPLAIN }), 'Explain is hidden once asked for').toBeNull()
    expect(within(result).getByRole('button', { name: 'Continue' })).toBeInTheDocument()
  })

  it('shows no tip line when there is no tip', async () => {
    reply('exercise_submit', () => WRONG)
    reply('explain_answer', () => ({ text: 'Té means tea; the sentence asks for a coffee.', tip: null, cached: true }))
    const result = await answerCoffee('Me gusta un té con leche')
    fireEvent.click(within(result).getByRole('button', { name: EXPLAIN }))
    await expectText('Té means tea; the sentence asks for a coffee.')
    expect(hasText('Tip:')).toBe(false)
  })

  it('offline shows that explanations need a connection or a local model', async () => {
    reply('exercise_submit', () => ALMOST)
    reply('explain_answer', () => Promise.reject(appError('offline', EXPLAIN_OFFLINE)))
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: EXPLAIN }))
    await expectText(EXPLAIN_OFFLINE)
  })

  it.each([
    ['busy', BUSY],
    ['ai_invalid', INVALID],
  ])('%s shows the message and Try again, which asks again', async (code, message) => {
    reply('exercise_submit', () => ALMOST)
    reply('explain_answer', () => Promise.reject(appError(code, message)))
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: EXPLAIN }))
    await expectText(message)
    reply('explain_answer', () => EXPLAINED)
    fireEvent.click(await screen.findByRole('button', { name: TRY_AGAIN }))
    await waitFor(() => expect(callsOf('explain_answer')).toHaveLength(2))
    expect(callsOf('explain_answer')[1]).toEqual({ session_id: SESSION, exercise_id: 'e1' })
    await expectText(`Tip: ${EXPLAINED.tip}`)
  })

  it('needs_consent opens the AI setup dialog', async () => {
    aiStatus = STATUS_UNSET
    reply('exercise_submit', () => ALMOST)
    reply('explain_answer', () => Promise.reject(appError('needs_consent', NOT_SET_UP)))
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: EXPLAIN }))
    expect(await screen.findByRole('dialog', { name: 'Set up AI features' })).toBeInTheDocument()
  })
})

// ---- acceptance tests 8 to 11: Not checked ----------------------------------------------------

describe('Not checked (acceptance tests 8 to 11)', () => {
  it('without a reason offers Check with AI, I was right and Continue', async () => {
    const result = await answerCoffee()
    await expectText(NOT_CHECKED, result)
    for (const name of [CHECK_WITH_AI, I_WAS_RIGHT, 'Continue']) {
      expect(within(result).getByRole('button', { name })).toBeInTheDocument()
    }
    expect(within(result).queryByRole('button', { name: EXPLAIN })).toBeNull()
  })

  it('Check with AI calls exercise_check and shows the verdict', async () => {
    reply('exercise_check', () => ALMOST)
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: CHECK_WITH_AI }))
    await waitFor(() => expect(callsOf('exercise_check')).toEqual([{ session_id: SESSION, exercise_id: 'e1' }]))
    await expectText(`Correct answer: ${CORRECTED}`)
    await expectText(ALMOST_WHY)
  })

  it('Check with AI without a provider opens the AI setup dialog; Not now leaves the answer unchecked (I12)', async () => {
    aiStatus = STATUS_UNSET
    reply('exercise_check', () => Promise.reject(appError('needs_consent', NOT_SET_UP)))
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: CHECK_WITH_AI }))
    await waitFor(() => expect(callsOf('exercise_check')).toHaveLength(1))
    const dialog = await screen.findByRole('dialog', { name: 'Set up AI features' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Not now' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await expectText(NOT_CHECKED)
    expect(screen.getByRole('button', { name: I_WAS_RIGHT })).toBeInTheDocument()
    expect(callsOf('exercise_check')).toHaveLength(1)
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('Use my own key in the setup dialog closes it and leaves the answer unchecked', async () => {
    aiStatus = STATUS_UNSET
    reply('exercise_check', () => Promise.reject(appError('needs_consent', NOT_SET_UP)))
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: CHECK_WITH_AI }))
    const dialog = await screen.findByRole('dialog', { name: 'Set up AI features' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Use my own key' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await expectText(NOT_CHECKED)
    expect(screen.getByRole('button', { name: I_WAS_RIGHT })).toBeInTheDocument()
    expect(callsOf('exercise_check')).toHaveLength(1)
  })

  it('after the AI setup dialog, Continue checks the answer again', async () => {
    aiStatus = STATUS_UNSET
    let checks = 0
    reply('exercise_check', () => {
      checks += 1
      return checks === 1 ? Promise.reject(appError('needs_consent', NOT_SET_UP)) : ALMOST
    })
    reply('ai_consent_accept', () => {
      aiStatus = STATUS_PUBLIK
      return aiStatus
    })
    const result = await answerCoffee()
    fireEvent.click(within(result).getByRole('button', { name: CHECK_WITH_AI }))
    const dialog = await screen.findByRole('dialog', { name: 'Set up AI features' })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Continue with publik API' }))
    await waitFor(() => expect(callsOf('ai_consent_accept')).toHaveLength(1))
    // The first-run card of module 06 may follow; its Continue ends the setup.
    await waitFor(() => {
      const card = screen.queryByRole('dialog')
      const go = card ? within(card).queryByRole('button', { name: 'Continue' }) : null
      expect(callsOf('exercise_check').length >= 2 || go !== null).toBe(true)
    })
    if (callsOf('exercise_check').length < 2) {
      fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Continue' }))
    }
    await waitFor(() => expect(callsOf('exercise_check')).toHaveLength(2))
    expect(callsOf('exercise_check')[1]).toEqual({ session_id: SESSION, exercise_id: 'e1' })
    await expectText(`Correct answer: ${CORRECTED}`)
  })

  it('a 402 shows Not checked, the message, exactly one link and the way out, and no Check with AI', async () => {
    reply(
      'exercise_submit',
      () => notChecked('needs_credit', BALANCE, { top_up_url: 'https://publikhq.com/wallet/add-credit?from=402' }),
    )
    const result = await answerCoffee()
    await expectText(NOT_CHECKED, result)
    await expectText(BALANCE)
    await expectText(OR_ELSE)
    const links = controlsNamed(document.body, LINKS)
    expect(links, 'exactly one link').toHaveLength(1)
    expect(result.querySelectorAll('a[href]'), 'the web view opens no URL of its own').toHaveLength(0)
    expect(screen.queryByRole('button', { name: CHECK_WITH_AI })).toBeNull()
    expect(within(result).getByRole('button', { name: I_WAS_RIGHT })).toBeInTheDocument()
    expect(within(result).getByRole('button', { name: 'Continue' })).toBeInTheDocument()
    fireEvent.click(links[0])
    await waitFor(() => expect(callsOf('ai_open_link')).toEqual([{ kind: 'top_up' }]))
  })

  it('offline shows that free answers cannot be checked, and I was right', async () => {
    reply('exercise_submit', () => notChecked('offline', JUDGE_OFFLINE))
    const result = await answerCoffee()
    await expectText(NOT_CHECKED, result)
    await expectText(JUDGE_OFFLINE)
    expect(screen.queryByRole('button', { name: CHECK_WITH_AI })).toBeNull()
    fireEvent.click(within(result).getByRole('button', { name: I_WAS_RIGHT }))
    await waitFor(() => expect(callsOf('exercise_i_was_right')).toEqual([{ session_id: SESSION, exercise_id: 'e1' }]))
  })

  it('a timeout shows the message and Try again, which calls exercise_check', async () => {
    reply('exercise_submit', () => notChecked('busy', TIMED_OUT))
    reply('exercise_check', () => ALMOST)
    const result = await answerCoffee()
    await expectText(NOT_CHECKED, result)
    await expectText(TIMED_OUT)
    expect(within(result).getByRole('button', { name: I_WAS_RIGHT })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: TRY_AGAIN }))
    await waitFor(() => expect(callsOf('exercise_check')).toEqual([{ session_id: SESSION, exercise_id: 'e1' }]))
    await expectText(`Correct answer: ${CORRECTED}`)
  })

  it('an unusable AI answer shows the message with Try again and I was right', async () => {
    reply('exercise_submit', () => notChecked('ai_invalid', INVALID))
    reply('exercise_check', () => notChecked('ai_invalid', INVALID))
    const result = await answerCoffee()
    await expectText(NOT_CHECKED, result)
    await expectText(INVALID)
    expect(within(result).getByRole('button', { name: I_WAS_RIGHT })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: TRY_AGAIN }))
    await waitFor(() => expect(callsOf('exercise_check')).toHaveLength(1))
  })

  it('disconnected shows the message and Reconnect', async () => {
    reply('exercise_submit', () => notChecked('disconnected', DISCONNECTED))
    const result = await answerCoffee()
    await expectText(NOT_CHECKED, result)
    await expectText(DISCONNECTED)
    expect(within(result).getByRole('button', { name: I_WAS_RIGHT })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }))
    await waitFor(() => expect(callsOf('ai_reconnect_publik')).toHaveLength(1))
  })
})
