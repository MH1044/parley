// Module 06 UI acceptance tests (docs/modules/06-publik-and-providers.md, UI; acceptance tests 6,
// 8, 18, 21 and 24): the app is rendered whole, the Tauri bridge answers from a table of the spec's
// result shapes (AiStatus with every field), and every element is found by the roles and visible
// text of the spec's UI section and docs/PUBLIK-INTEGRATION.md §4.

import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import en from '../../strings/en.json'

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

const strings: Record<string, string> = en

// ---- the spec's texts --------------------------------------------------------------------------

/** `DISCLOSURE` of the publik API reference module, word for word. */
const DISCLOSURE =
  "This app can use publik API for its AI features: no key to paste, priced per use in dollars from a publik balance that starts at $0.00. Linking this computer to a publik account gives $0.05 of free use, once. Your prompts go through publik's servers to the model; you can switch to your own key at any time."
const DATA_PATH =
  "Parley sends only what the AI feature you use needs through publik's servers: the lesson topic and the words you know, the exercise and your answer, your conversation turn, the sentence you just recorded, the photo you picked, or the audio of a file you chose. Your progress, reviews and other recordings stay on this computer."
const WORKS_WITHOUT = 'Everything else in Parley works without AI.'
const CARD = 'Parley works without AI. Set it up for fresh lessons, answer checks and conversations.'
const DIALOG_BUTTONS = ['Continue with publik API', 'Use my own key', 'Use a local model', 'Not now']
const FRESH_LINE = '$0.00 · link this computer for $0.05 of free use'
const COST = 'A typical use costs about $0.01 on publik-balanced and $0.001 on publik-fast.'
const OR_ELSE = 'or use your own key or a local model in Settings'

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

const STATUS_ANONYMOUS = {
  ...STATUS_UNSET,
  provider: 'publik',
  needs_consent: false,
  claim_state: 'anonymous',
  balance_micros: 0,
  balance_line: FRESH_LINE,
  cost_sentence: COST,
  claim_url: 'https://publikhq.com/claim/5f0c8a52',
  add_credit_url: 'https://publikhq.com/wallet/add-credit',
  top_up_url: 'https://publikhq.com/claim/5f0c8a52?from=wallet',
}

const STATUS_CLAIMED = {
  ...STATUS_ANONYMOUS,
  claim_state: 'claimed',
  balance_micros: 3_120_000,
  balance_line: '$3.12 of publik balance',
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

/** The path with a locked skill and a story not open yet: both are drawn faded. */
const PATH_WITH_LOCKED = {
  ...PATH,
  units: [
    {
      ...PATH.units[0],
      skills: [
        ...PATH.units[0].skills,
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
      story: { skill_id: 'es.a1.u01.s02', available: false },
    },
  ],
}

const LOCAL_MODELS = [
  { name: 'llama3.1:8b', bytes: 4_700_000_000 },
  { name: 'llama3.2:3b', bytes: 2_000_000_000 },
]

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

/** The status every AI command answers with; a test changes it to follow its story. */
let aiStatus: Record<string, unknown> = STATUS_UNSET

/** Delivers `payload` to every handler registered for `event`, as the core's emit would. */
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
  aiStatus = STATUS_UNSET
  reply('app_start', () => ({ onboarded: true, active_course: 'es', notice: null }))
  reply('app_info', () => ({ version: '9.9.9', data_dir: 'parley-data-folder' }))
  reply('settings_get', () => SETTINGS)
  reply('settings_set', (args) => ({ ...SETTINGS, ...(args.patch as Record<string, unknown>) }))
  reply('course_list', () => COURSES)
  reply('course_start', (args) => ({ ...COURSES[0], lang: args.lang, started: true, active: true }))
  reply('pack_status', () => PACK_AVAILABLE)
  reply('path_get', () => PATH)
  reply('stats_get', () => ({ lang: 'es', empty: true, streak: 0, freezes: 0, days: [] }))
  reply('ai_status', () => aiStatus)
  reply('ai_consent_decline', () => aiStatus)
  reply('ai_open_link', () => null)
  reply('ai_reconnect_publik', () => {
    aiStatus = { ...aiStatus, connected: true }
    return aiStatus
  })
  reply('ai_check_connection', () => ({
    ok: true,
    charge_micros: 17,
    message: 'Connected. This check cost $0.000017.',
  }))
}

function notOnboarded() {
  reply('app_start', () => ({ onboarded: false, active_course: null, notice: null }))
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

/** Some element inside `container` whose whole text is exactly `text`. */
async function expectExactText(text: string, container: HTMLElement) {
  await waitFor(() => {
    const exact = within(container)
      .queryAllByText(textOf(text))
      .filter((element) => squash(element.textContent) === text)
    expect(exact.length, `an element saying exactly: ${text}`).toBeGreaterThan(0)
  })
}

function expectNoDialog() {
  expect(screen.queryByRole('dialog')).toBeNull()
}

async function heading(name: string | RegExp, level?: number) {
  return screen.findByRole('heading', level === undefined ? { name } : { name, level })
}

async function setupDialog() {
  return screen.findByRole('dialog', { name: 'Set up AI features' })
}

/** The error banner that says `message`. */
async function bannerSaying(message: string): Promise<HTMLElement> {
  return waitFor(() => {
    const found = screen.queryAllByRole('alert').find((alert) => squash(alert.textContent).includes(message))
    expect(found, `an alert saying: ${message}`).toBeDefined()
    return found as HTMLElement
  })
}

/** The buttons and links inside `container` named one of `names`. */
function controlsNamed(container: HTMLElement, names: string[]) {
  return names.flatMap((name) => [
    ...within(container).queryAllByRole('button', { name }),
    ...within(container).queryAllByRole('link', { name }),
  ])
}

async function openPath() {
  render(<App />)
  await expectText('4 day streak')
}

async function openAiSettings() {
  render(<App />)
  fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
  await heading('Settings')
  fireEvent.click(await screen.findByRole('button', { name: 'AI' }))
  await waitFor(() => expect(callsOf('ai_status')).toContainEqual({ refresh: true }))
}

beforeEach(() => {
  answerDefaults()
})

afterEach(() => {
  cleanup()
})

// ---- acceptance test 18: consent at the first AI action, never at launch ---------------------

describe('Consent dialog (acceptance test 18, invariant I12)', () => {
  it('launch shows the Set up AI card on the path and no dialog', async () => {
    await openPath()
    await expectText(CARD)
    expect(await screen.findByRole('button', { name: 'Set up AI' })).toBeInTheDocument()
    expectNoDialog()
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('onboarding shows no AI dialog and asks for no consent', async () => {
    notOnboarded()
    render(<App />)
    await heading('Welcome to Parley', 1)
    expectNoDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await heading('Choose a language', 1)
    expectNoDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Spanish' }))
    await heading('Course pack', 1)
    expectNoDialog()
    expect(screen.queryByRole('button', { name: 'Set up AI' })).toBeNull()
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('Set up AI opens the dialog with the exact title, sentences and four buttons', async () => {
    expect(strings['ai.setup.disclosure']).toBe(DISCLOSURE)
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Set up AI' }))
    const dialog = await setupDialog()
    await expectText('Set up AI features', dialog)
    await expectExactText(DATA_PATH, dialog)
    await expectExactText(strings['ai.setup.disclosure'], dialog)
    await expectExactText(WORKS_WITHOUT, dialog)
    for (const name of DIALOG_BUTTONS) {
      expect(within(dialog).getByRole('button', { name })).toBeInTheDocument()
    }
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('on the path the dialog sits on the backdrop, last on the page, after the locked skill and story', async () => {
    // Module 04, Design notes: dialogs sit above everything. A faded button paints as its own
    // layer, so the dialog comes after every one of them on the path.
    reply('path_get', () => PATH_WITH_LOCKED)
    await openPath()
    const locked = await screen.findByRole('button', { name: 'Names' })
    const story = screen.getByRole('button', { name: 'Read a story' })
    expect(story).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Set up AI' }))
    const dialog = await setupDialog()
    const backdrop = dialog.parentElement as HTMLElement
    expect(backdrop).toHaveClass('dialog-backdrop')
    const home = backdrop.closest('main')
    expect(home).toHaveClass('path')
    expect(home?.lastElementChild, 'the backdrop is the last element of the path').toBe(backdrop)
    for (const button of [locked, story]) {
      expect(button.compareDocumentPosition(backdrop) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    }
    const after = Array.from(document.body.querySelectorAll('*')).filter(
      (element) =>
        !backdrop.contains(element) && backdrop.compareDocumentPosition(element) & Node.DOCUMENT_POSITION_FOLLOWING,
    )
    expect(after.map((element) => element.outerHTML), 'nothing on the page comes after the dialog').toEqual([])
  })

  it('Not now calls ai_consent_decline, sends nothing else and closes', async () => {
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Set up AI' }))
    const dialog = await setupDialog()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Not now' }))
    await waitFor(() => expect(callsOf('ai_consent_decline')).toHaveLength(1))
    await waitFor(() => expectNoDialog())
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('a failed Continue with publik API shows its message with the own key and the local model', async () => {
    const message = 'publik API could not set up this computer (503). Use your own key for now.'
    reply('ai_consent_accept', failWith(appError('provider_error', message)))
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Set up AI' }))
    const dialog = await setupDialog()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Continue with publik API' }))
    await waitFor(() => expect(callsOf('ai_consent_accept')).toHaveLength(1))
    await expectText(message)
    const shown = screen.getByRole('dialog')
    expect(within(shown).getByRole('button', { name: 'Use my own key' })).toBeInTheDocument()
    expect(within(shown).getByRole('button', { name: 'Use a local model' })).toBeInTheDocument()
  })

  it('an AI command that fails with needs_consent opens the dialog', async () => {
    reply('ai_check_connection', failWith(appError('needs_consent', 'AI features are not set up yet.')))
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Check connection' }))
    await waitFor(() => expect(callsOf('ai_check_connection')).toHaveLength(1))
    await setupDialog()
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })
})

// ---- acceptance test 21: the first-run card ---------------------------------------------------

describe('First-run card (acceptance test 21)', () => {
  async function acceptWith(status: Record<string, unknown>) {
    reply('ai_consent_accept', () => {
      aiStatus = status
      return status
    })
    await openPath()
    fireEvent.click(await screen.findByRole('button', { name: 'Set up AI' }))
    const dialog = await setupDialog()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Continue with publik API' }))
    await waitFor(() => expect(callsOf('ai_consent_accept')).toHaveLength(1))
    return screen.findByRole('dialog')
  }

  it('at $0.00 anonymous shows the balance line, the cost sentence and Sign in with publik', async () => {
    const card = await acceptWith(STATUS_ANONYMOUS)
    await expectText(FRESH_LINE, card)
    await expectText(COST, card)
    const signIn = await within(card).findByRole('button', { name: 'Sign in with publik' })
    expect(within(card).queryByRole('button', { name: 'Add a plan or pack' })).toBeNull()
    for (const name of ['Use my own key', 'Use a local model', 'Continue']) {
      expect(within(card).getByRole('button', { name })).toBeInTheDocument()
    }
    fireEvent.click(signIn)
    await waitFor(() => expect(callsOf('ai_open_link')).toEqual([{ kind: 'claim' }]))
    fireEvent.click(within(card).getByRole('button', { name: 'Continue' }))
    await waitFor(() => expectNoDialog())
  })

  it('once claimed shows Add a plan or pack, which opens the add-credit link', async () => {
    const card = await acceptWith(STATUS_CLAIMED)
    await expectText('$3.12 of publik balance', card)
    const addCredit = await within(card).findByRole('button', { name: 'Add a plan or pack' })
    expect(within(card).queryByRole('button', { name: 'Sign in with publik' })).toBeNull()
    fireEvent.click(addCredit)
    await waitFor(() => expect(callsOf('ai_open_link')).toEqual([{ kind: 'add_credit' }]))
  })
})

// ---- the AI chip on the path --------------------------------------------------------------------

describe('AI chip on the path', () => {
  it.each([
    ['publik', 'AI: publik API'],
    ['own_key', 'AI: your own key'],
    ['local', 'AI: local model'],
  ])('with the provider %s it says %s', async (provider, chip) => {
    aiStatus =
      provider === 'publik'
        ? STATUS_CLAIMED
        : { ...STATUS_UNSET, provider, needs_consent: false, own_key_stored: provider === 'own_key' }
    await openPath()
    await expectText(chip)
    if (provider === 'publik') {
      await expectText('$3.12 of publik balance')
    }
    expect(screen.queryAllByText(textOf(CARD))).toHaveLength(0)
    expect(screen.queryByRole('button', { name: 'Set up AI' })).toBeNull()
  })

  it('shows nothing AI-related while ai_status fails', async () => {
    fake.answers.delete('ai_status')
    await openPath()
    await waitFor(() => expect(callsOf('ai_status').length).toBeGreaterThan(0))
    await expectText('4 day streak')
    expect(screen.queryByRole('button', { name: 'Set up AI' })).toBeNull()
    expect(screen.queryAllByText(textOf(CARD))).toHaveLength(0)
    expect(screen.queryAllByText(textOf('AI:'))).toHaveLength(0)
  })
})

// ---- the event ai://status ------------------------------------------------------------------------

describe('The event ai://status', () => {
  it('a status the core emits replaces the Set up AI card with the chip and its balance line', async () => {
    await openPath()
    await expectText(CARD)
    await waitFor(() => expect(fake.listeners.get('ai://status')?.length ?? 0).toBeGreaterThan(0))

    emitEvent('ai://status', STATUS_CLAIMED)
    await expectText('AI: publik API')
    await expectText('$3.12 of publik balance')
    await waitFor(() => expect(screen.queryAllByText(textOf(CARD))).toHaveLength(0))
    expect(screen.queryByRole('button', { name: 'Set up AI' })).toBeNull()
  })
})

// ---- acceptance test 24: Settings > AI ---------------------------------------------------------

describe('Settings > AI (acceptance test 24)', () => {
  it('the AI tab follows Learning, refreshes the status and lists the three providers, the chosen one In use', async () => {
    aiStatus = STATUS_CLAIMED
    await openAiSettings()
    const learning = screen.getByRole('button', { name: 'Learning' })
    const aiTab = screen.getByRole('button', { name: 'AI' })
    expect(learning.compareDocumentPosition(aiTab) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()

    const publik = await heading(/^publik API/)
    const own = await heading(/^Your own key/)
    const local = await heading(/^Local model/)
    expect(publik.compareDocumentPosition(own) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(own.compareDocumentPosition(local) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()

    const inUse = screen.getAllByText(textOf('In use'))
    expect(inUse).toHaveLength(1)
    const after = Node.DOCUMENT_POSITION_FOLLOWING | Node.DOCUMENT_POSITION_CONTAINED_BY
    expect(publik.compareDocumentPosition(inUse[0]) & after, 'In use follows publik API').toBeTruthy()
    expect(inUse[0].compareDocumentPosition(own) & Node.DOCUMENT_POSITION_FOLLOWING, 'and comes before Your own key').toBeTruthy()

    await expectText('$3.12 of publik balance')
    expect(screen.getByRole('button', { name: 'Add a plan or pack' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Disconnect' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Use publik API' })).toBeNull()
  })

  it('Check connection shows the result message', async () => {
    aiStatus = STATUS_CLAIMED
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Check connection' }))
    await waitFor(() => expect(callsOf('ai_check_connection')).toHaveLength(1))
    await expectText('Connected. This check cost $0.000017.')
  })

  it('Use publik API goes through the dialog while consent is needed', async () => {
    aiStatus = { ...STATUS_UNSET, provider: 'own_key', needs_consent: true, own_key_stored: true }
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Use publik API' }))
    await setupDialog()
    expect(callsOf('ai_consent_accept')).toHaveLength(0)
  })

  it('Use publik API accepts directly when a key already resolves', async () => {
    aiStatus = { ...STATUS_CLAIMED, provider: 'own_key', own_key_stored: true }
    reply('ai_consent_accept', () => STATUS_CLAIMED)
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Use publik API' }))
    await waitFor(() => expect(callsOf('ai_consent_accept')).toHaveLength(1))
  })

  it('the own key: a password field never filled back, Save my key and Remove my key', async () => {
    aiStatus = { ...STATUS_UNSET, provider: 'own_key', needs_consent: false, own_key_stored: true }
    reply('settings_get', () => ({
      ...SETTINGS,
      ai: { ...SETTINGS.ai, provider: 'own_key', own_key: { ...SETTINGS.ai.own_key, base_url: 'https://api.example/v1', chat_model: 'gpt-own-mini' } },
    }))
    reply('ai_set_own_key', () => aiStatus)
    reply('ai_clear_own_key', () => ({ ...aiStatus, own_key_stored: false }))
    await openAiSettings()
    const key = (await screen.findByLabelText('Key')) as HTMLInputElement
    expect(key.type).toBe('password')
    expect(key.value).toBe('')

    fireEvent.change(screen.getByLabelText('Base URL'), { target: { value: 'https://api.example/v1' } })
    fireEvent.change(key, { target: { value: 'sk-own-a' } })
    fireEvent.change(screen.getByLabelText('Chat model'), { target: { value: 'gpt-own-mini' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save my key' }))
    await waitFor(() => expect(callsOf('ai_set_own_key')).toHaveLength(1))
    expect(callsOf('ai_set_own_key')[0]).toMatchObject({
      base_url: 'https://api.example/v1',
      key: 'sk-own-a',
      chat_model: 'gpt-own-mini',
    })

    fireEvent.click(screen.getByRole('button', { name: 'Remove my key' }))
    await waitFor(() => expect(callsOf('ai_clear_own_key')).toHaveLength(1))
  })

  it('the local model: the default server address and Find models', async () => {
    reply('ai_local_probe', () => LOCAL_MODELS)
    await openAiSettings()
    const address = (await screen.findByLabelText('Server address')) as HTMLInputElement
    expect(address.value).toBe('http://localhost:11434/v1')
    fireEvent.click(screen.getByRole('button', { name: 'Find models' }))
    await waitFor(() => expect(callsOf('ai_local_probe')).toEqual([{ base_url: 'http://localhost:11434/v1' }]))
    await expectText('llama3.1:8b')
    await expectText('llama3.2:3b')
    expect(screen.getAllByRole('button', { name: 'Use this model' }).length).toBeGreaterThan(0)
  })
})

// ---- acceptance tests 6 and 8: the error banners ------------------------------------------------

describe('Error banner (acceptance tests 6 and 8)', () => {
  it.each([
    [
      'anonymous',
      STATUS_ANONYMOUS,
      'This computer has $0.00 of publik balance. Link it to a publik account for $0.05 of free use.',
      'https://publikhq.com/claim/5f0c8a52?from=402',
      'Sign in with publik',
    ],
    [
      'claimed',
      STATUS_CLAIMED,
      'Your publik balance is $0.00. Add a plan or pack to keep using AI features.',
      'https://publikhq.com/wallet/add-credit?from=402',
      'Add a plan or pack',
    ],
  ])('402 while %s: the message, exactly one link button and the way out', async (_state, status, message, topUp, label) => {
    aiStatus = status
    reply('ai_check_connection', failWith(appError('needs_credit', message, { top_up_url: topUp })))
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Check connection' }))
    const banner = await bannerSaying(message)
    await expectText(OR_ELSE, banner)
    const links = controlsNamed(banner, ['Sign in with publik', 'Add a plan or pack'])
    expect(links, 'exactly one link').toHaveLength(1)
    expect(squash(links[0].textContent)).toBe(label)
    expect(banner.querySelectorAll('a[href]'), 'the web view opens no URL of its own').toHaveLength(0)
    fireEvent.click(links[0])
    await waitFor(() => expect(callsOf('ai_open_link')).toEqual([{ kind: 'top_up' }]))
  })

  it('disconnected: the message and Reconnect, which calls ai_reconnect_publik', async () => {
    aiStatus = STATUS_CLAIMED
    reply('ai_check_connection', failWith(appError('disconnected', 'publik API is disconnected.')))
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Check connection' }))
    const banner = await bannerSaying('publik API is disconnected.')
    fireEvent.click(within(banner).getByRole('button', { name: 'Reconnect' }))
    await waitFor(() => expect(callsOf('ai_reconnect_publik')).toHaveLength(1))
  })

  it.each([
    ['busy', 'publik API is busy; try again in a moment'],
    ['ai_invalid', 'The AI answer could not be used.'],
  ])('%s: the message and Try again, which runs the action again', async (code, message) => {
    aiStatus = STATUS_CLAIMED
    reply('ai_check_connection', failWith(appError(code, message)))
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Check connection' }))
    const banner = await bannerSaying(message)
    fireEvent.click(within(banner).getByRole('button', { name: 'Try again' }))
    await waitFor(() => expect(callsOf('ai_check_connection')).toHaveLength(2))
  })

  it('any other code shows the message alone', async () => {
    aiStatus = STATUS_CLAIMED
    const message = 'publik API 500: The model provider failed.'
    reply('ai_check_connection', failWith(appError('provider_error', message)))
    await openAiSettings()
    fireEvent.click(await screen.findByRole('button', { name: 'Check connection' }))
    const banner = await bannerSaying(message)
    for (const name of ['Try again', 'Reconnect', 'Sign in with publik', 'Add a plan or pack']) {
      expect(within(banner).queryByRole('button', { name })).toBeNull()
    }
  })
})
