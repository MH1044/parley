import { useEffect, useRef, useState } from 'react'
import {
  asAppError,
  errorMessage,
  micStart,
  micStop,
  onMicLevel,
  onMicLimit,
  recordingPlay,
  ttsPlay,
  type AiStatus,
  type AppError,
  type Recording,
  type SpeakResult,
  type WordResult,
} from '../api/commands'
import { t } from '../strings'
import AiBanner from './AiBanner'
import LevelMeter from './LevelMeter'
import PlayButton from './PlayButton'

/** The longest take of an exercise, in seconds. */
const MAX_S = 15
/** The meter's floor, in dBFS. */
const FLOOR = -60

export interface SpeakProps {
  /** The take recorded for this exercise, if any. */
  take: Recording | null
  /** A new take, or none while one is being recorded. */
  onTake: (take: Recording | null) => void
  /** The checked take: what Parley heard, word by word. */
  result: SpeakResult | null
  /** `speaking.pass_threshold`, for the score line. */
  passMark: number
  /** What went wrong with the microphone or the check. */
  error: AppError | null
  onError: (error: AppError | null) => void
  /** A check is running: nothing can be recorded meanwhile. */
  busy: boolean
  /** The keyboard belongs to the exercise (no dialog is open). */
  keyboard: boolean
  /** The AI status's claim state, for the label of a 402's one link. */
  claimState?: string | null
  /** Use the local model: the same take checked by local Whisper. */
  onUseLocal: () => void
  /** Try again after a busy provider or an unusable answer: the same take checked again. */
  onRetry: () => void
  /** Try again after a result: a new take, checked again, rated nothing. */
  onTryAgain: () => void
  /** Can't speak now: this and every later speaking exercise of the lesson are skipped. */
  onCantSpeak: () => void
  /** Reconnect succeeded: the new status. */
  onStatus?: (status: AiStatus) => void
}

interface Props extends SpeakProps {
  /** The sentence to say, in the course language. */
  text: string
  /** Its English. */
  promptEn: string
  /** The course language's code. */
  lang: string
  sessionId: string | null
  autoPlay: boolean
}

/** An error the bridge rejected with, as an AppError. */
function toAppError(reason: unknown): AppError {
  return asAppError(reason) ?? { code: '', message: errorMessage(reason), action: null }
}

const ICONS: Record<WordResult['status'], string> = {
  ok: '✓',
  missed: '✗',
  substituted: '≠',
}

/** "{word}: right", "{word}: missed" or "{word}: heard {heard}". */
function wordLabel(word: WordResult): string {
  switch (word.status) {
    case 'ok':
      return t('speak.word_ok', { word: word.expected })
    case 'missed':
      return t('speak.word_missed', { word: word.expected })
    case 'substituted':
      return t('speak.word_substituted', { word: word.expected, heard: word.heard ?? '' })
  }
}

/** Whether a key event comes from a place where Space types a space. */
function inTextField(target: EventTarget | null): boolean {
  if (target instanceof HTMLTextAreaElement) return true
  if (target instanceof HTMLInputElement) {
    return !['button', 'checkbox', 'radio', 'range', 'submit', 'reset'].includes(target.type)
  }
  return target instanceof HTMLElement && target.isContentEditable
}

/**
 * Say this (docs/modules/10-speaking-and-pronunciation.md, UI): the sentence, its English, Play
 * and Slow, and Hold to talk: pressing the button or Space starts a take of at most 15 s, letting
 * go (or the take's limit) stops it. While recording, the level meter follows the microphone.
 * After a take: its length and Replay my take. After the check: what Parley heard, each expected
 * word with its colour, icon and text, the extra words, the score against the pass mark, Play the
 * voice and Try again. Errors show as the AI banner does, with Use the local model when it is
 * downloaded.
 */
export default function SpeakExercise({
  text,
  promptEn,
  lang,
  sessionId,
  autoPlay,
  take,
  onTake,
  result,
  passMark,
  error,
  onError,
  busy,
  keyboard,
  claimState,
  onUseLocal,
  onRetry,
  onTryAgain,
  onCantSpeak,
  onStatus,
}: Props) {
  const [recording, setRecording] = useState(false)
  const [level, setLevel] = useState(FLOOR)
  const [voiceError, setVoiceError] = useState<string | null>(null)
  // A take is being held: set at the press, cleared at the release.
  const holding = useRef(false)
  // Whether the take started, once mic_start has answered.
  const started = useRef<Promise<boolean> | null>(null)
  const mounted = useRef(true)
  const disabled = busy || result !== null

  function begin() {
    if (holding.current || disabled) return
    holding.current = true
    setRecording(true)
    setLevel(FLOOR)
    onError(null)
    onTake(null)
    started.current = micStart(MAX_S).then(
      () => true,
      (reason: unknown) => {
        holding.current = false
        if (mounted.current) {
          setRecording(false)
          onError(toAppError(reason))
        }
        return false
      },
    )
  }

  async function end() {
    if (!holding.current) return
    holding.current = false
    const ok = await (started.current ?? Promise.resolve(false))
    if (!mounted.current) return
    setRecording(false)
    if (!ok) return
    try {
      const recorded = await micStop()
      if (mounted.current && recorded) onTake(recorded)
    } catch (reason) {
      if (mounted.current) onError(toAppError(reason))
    }
  }

  // The latest press and release, for the listeners registered once.
  const actions = useRef({ begin, end, disabled })
  useEffect(() => {
    actions.current = { begin, end, disabled }
  })

  useEffect(() => {
    mounted.current = true
    const levels = onMicLevel(({ dbfs }) => {
      if (mounted.current && holding.current) setLevel(Math.round(dbfs))
    })
    // The take stops at its limit as a release would; the window keeps no timer of its own.
    const limits = onMicLimit(() => void actions.current.end())
    return () => {
      mounted.current = false
      void levels.then((stop) => stop())
      void limits.then((stop) => stop())
    }
  }, [])

  // Space: held, it records; released, it stops. Not a repeat, not in a text field.
  useEffect(() => {
    if (!keyboard) return
    function down(event: KeyboardEvent) {
      if (event.key !== ' ' || inTextField(event.target)) return
      // Space is left to a focused button while there is nothing to record.
      if (actions.current.disabled && !holding.current) return
      event.preventDefault()
      if (event.repeat) return
      actions.current.begin()
    }
    function up(event: KeyboardEvent) {
      if (event.key !== ' ' || inTextField(event.target) || !holding.current) return
      event.preventDefault()
      void actions.current.end()
    }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
    }
  }, [keyboard])

  async function replay() {
    if (!take) return
    try {
      await recordingPlay(take.id)
    } catch (reason) {
      onError(toAppError(reason))
    }
  }

  async function playVoice() {
    setVoiceError(null)
    try {
      await ttsPlay(text, lang, false, sessionId)
    } catch (reason) {
      const failed = asAppError(reason)
      if (failed?.code === 'no_voice') setVoiceError(failed.message)
    }
  }

  const useLocal = error?.action?.use_local === true
  const retry = error?.code === 'busy' || error?.code === 'ai_invalid' ? onRetry : undefined
  // Can't speak now sits in the 402's alert while it shows, and under the exercise otherwise.
  const cantSpeakInAlert = error?.code === 'needs_credit'
  const cantSpeak = (
    <button type="button" className="link" disabled={busy} onClick={onCantSpeak}>
      {t('speak.cant_speak')}
    </button>
  )

  return (
    <div className="exercise speak">
      <h1>{t('exercise.speak')}</h1>
      <p className="prompt" lang={lang}>
        {text}
      </p>
      <p className="prompt-en" lang="en">
        {promptEn}
      </p>
      <PlayButton text={text} lang={lang} sessionId={sessionId} autoPlay={autoPlay} />
      <div className="actions">
        <button
          type="button"
          className={recording ? 'hold recording' : 'hold'}
          aria-label={t('speak.hold_label')}
          aria-pressed={recording}
          disabled={disabled && !recording}
          onPointerDown={(event) => {
            event.preventDefault()
            begin()
          }}
          onPointerUp={() => void end()}
          onPointerCancel={() => void end()}
          onPointerLeave={() => void end()}
        >
          {recording ? t('speak.recording') : t('speak.hold')}
        </button>
      </div>
      {recording && <LevelMeter dbfs={level} />}
      {take && !recording && (
        <div className="take">
          <p>{t('speak.take', { s: (take.duration_ms / 1000).toFixed(1) })}</p>
          <button type="button" onClick={() => void replay()}>
            {t('speak.replay')}
          </button>
        </div>
      )}
      {result && (
        <div className="speak-result">
          <p className="heard" lang={lang}>
            {t('speak.heard', { heard: result.heard })}
          </p>
          <ul aria-label={t('speak.words_label')} className="speak-words">
            {result.words.map((word, i) => (
              <li key={`${i}-${word.expected}`} className={`word-${word.status}`}>
                <span aria-hidden="true" className="icon">
                  {ICONS[word.status]}
                </span>{' '}
                <span lang={lang}>{wordLabel(word)}</span>
              </li>
            ))}
          </ul>
          {result.extra.length > 0 && (
            <p className="extra" lang={lang}>
              {t('speak.also_heard', { extra: result.extra.join(', ') })}
            </p>
          )}
          <p className="score">
            {t('speak.score', {
              score: Math.round(result.score * 100),
              pass: Math.round(passMark * 100),
            })}
          </p>
          {result.notice && <p className="notice">{result.notice}</p>}
          <div className="actions">
            <button type="button" onClick={() => void playVoice()}>
              {t('speak.play_voice')}
            </button>
            <button type="button" disabled={busy} onClick={onTryAgain}>
              {t('speak.try_again')}
            </button>
          </div>
          {voiceError && <p className="error">{voiceError}</p>}
        </div>
      )}
      {error && (
        <AiBanner error={error} claimState={claimState} onRetry={retry} onStatus={onStatus}>
          {useLocal && (
            <button type="button" disabled={busy} onClick={onUseLocal}>
              {t('speak.use_local')}
            </button>
          )}
          {cantSpeakInAlert && cantSpeak}
        </AiBanner>
      )}
      {!result && !cantSpeakInAlert && <div className="actions">{cantSpeak}</div>}
    </div>
  )
}
