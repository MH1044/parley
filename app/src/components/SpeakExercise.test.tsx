// The speaking exercise on its own: hold to talk with the button and with Space, the take's limit,
// the level meter, Replay my take, and the words heard with their classes, icons and labels.

import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AppError, Recording, SpeakResult } from '../api/commands'

const bridge = vi.hoisted(() => ({
  calls: [] as { command: string; args: Record<string, unknown> }[],
  listeners: new Map<string, ((event: { payload: unknown }) => void)[]>(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    bridge.calls.push({ command, args: args ?? {} })
    if (command === 'mic_stop') return { id: 'take-1', duration_ms: 1_250 }
    return null
  }),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    bridge.listeners.set(event, [...(bridge.listeners.get(event) ?? []), handler])
    return () => {}
  }),
}))

import SpeakExercise from './SpeakExercise'

function emit(event: string, payload: unknown) {
  for (const handler of bridge.listeners.get(event) ?? []) handler({ payload })
}

function commands(): string[] {
  return bridge.calls.map((call) => call.command).filter((name) => name !== 'tts_play')
}

const RESULT: SpeakResult = {
  words: [
    { expected: 'Yo', heard: 'yo', status: 'ok' },
    { expected: 'soy', heard: 'es', status: 'substituted' },
    { expected: 'estudiante', heard: null, status: 'missed' },
  ],
  extra: ['señor'],
  score: 1 / 3,
  pass: false,
  heard: 'yo es señor',
  grade: {
    verdict: 'wrong',
    tags: [],
    message: null,
    corrected: 'Yo soy estudiante.',
    words: ['soy', 'estudiante'],
    rating: 1,
  },
  rated: true,
  transcriber: 'provider',
  cached: false,
  notice: null,
}

function view(overrides: { result?: SpeakResult | null; take?: Recording | null; error?: AppError | null } = {}) {
  const onTake = vi.fn()
  const onError = vi.fn()
  const onTryAgain = vi.fn()
  const onCantSpeak = vi.fn()
  render(
    <SpeakExercise
      text="Yo soy estudiante."
      promptEn="I am a student."
      lang="es"
      sessionId="s-1"
      autoPlay={false}
      take={overrides.take ?? null}
      onTake={onTake}
      result={overrides.result ?? null}
      passMark={0.8}
      error={overrides.error ?? null}
      onError={onError}
      busy={false}
      keyboard
      onUseLocal={() => {}}
      onRetry={() => {}}
      onTryAgain={onTryAgain}
      onCantSpeak={onCantSpeak}
    />,
  )
  return { onTake, onError, onTryAgain, onCantSpeak }
}

beforeEach(() => {
  bridge.calls.length = 0
  bridge.listeners.clear()
})

afterEach(() => cleanup())

describe('SpeakExercise', () => {
  it('records while the button is held and keeps the take', async () => {
    const { onTake } = view()
    const hold = screen.getByRole('button', { name: 'Hold to talk, or hold Space' })
    fireEvent.pointerDown(hold)
    expect(hold).toHaveTextContent('Recording…')
    await waitFor(() => expect(screen.getByRole('meter', { name: 'Microphone level' })).toBeTruthy())
    act(() => emit('mic://level', { dbfs: -12.4 }))
    expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe('-12')
    fireEvent.pointerUp(hold)
    await waitFor(() => expect(onTake).toHaveBeenCalledWith({ id: 'take-1', duration_ms: 1_250 }))
    expect(commands()).toEqual(['mic_start', 'mic_stop'])
    expect(bridge.calls[0].args).toEqual({ max_s: 15 })
  })

  it('records while Space is held, and stops at the limit', async () => {
    const { onTake } = view()
    fireEvent.keyDown(window, { key: ' ' })
    fireEvent.keyDown(window, { key: ' ', repeat: true })
    await waitFor(() => expect(commands()).toEqual(['mic_start']))
    act(() => emit('mic://limit', {}))
    await waitFor(() => expect(onTake).toHaveBeenCalled())
    fireEvent.keyUp(window, { key: ' ' })
    expect(commands()).toEqual(['mic_start', 'mic_stop'])
  })

  it('shows the take and replays it', async () => {
    view({ take: { id: 'take-1', duration_ms: 1_250 } })
    expect(screen.getByText('Your take: 1.3 s')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Replay my take' }))
    await waitFor(() =>
      expect(bridge.calls).toContainEqual({ command: 'recording_play', args: { recording_id: 'take-1' } }),
    )
  })

  it('lists each word with its class, icon and label', () => {
    const { onTryAgain } = view({ result: RESULT })
    expect(screen.getByText('Parley heard: yo es señor')).toBeTruthy()
    const list = screen.getByRole('list', { name: 'What Parley heard' })
    const items = list.querySelectorAll('li')
    expect([...items].map((item) => item.className)).toEqual([
      'word-ok',
      'word-substituted',
      'word-missed',
    ])
    expect(screen.getByText('soy: heard es').closest('li')?.textContent).toBe('≠ soy: heard es')
    expect(screen.getByText('Yo: right')).toBeTruthy()
    expect(screen.getByText('estudiante: missed')).toBeTruthy()
    expect(screen.getByText('Also heard: señor')).toBeTruthy()
    expect(screen.getByText('Score 33% (pass mark 80%)')).toBeTruthy()
    expect(screen.queryByRole('button', { name: "Can't speak now" })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(onTryAgain).toHaveBeenCalled()
  })

  it('offers the local model and Can not speak now in a 402', () => {
    const { onCantSpeak } = view({
      error: {
        code: 'needs_credit',
        message: "Can't check speaking right now. Add credit to keep going.",
        action: { top_up_url: 'https://example.invalid/top-up', use_local: true },
      },
    })
    const alert = screen.getByRole('alert')
    expect(alert.textContent).toContain("Can't check speaking right now.")
    expect(screen.getByRole('button', { name: 'Use the local model' })).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: "Can't speak now" }))
    expect(onCantSpeak).toHaveBeenCalled()
  })
})
