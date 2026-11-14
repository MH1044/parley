import { useEffect, useId, useRef, useState } from 'react'
import {
  asAppError,
  errorMessage,
  exerciseCheck,
  exerciseIWasRight,
  exerciseSubmit,
  explainAnswer,
  lessonFinish,
  onAiStatus,
  sessionSkip,
  settingsGet,
  speakCheck,
  speakingStatus,
  ttsPrefetch,
  type AppError,
  type Exercise,
  type Grade,
  type LessonStarted,
  type LessonSummary,
  type Recording,
  type SpeakResult,
} from '../../api/commands'
import AiBanner from '../../components/AiBanner'
import AiSetupDialog from '../../components/AiSetupDialog'
import PlayButton from '../../components/PlayButton'
import ResultSheet, { type ExplainView } from '../../components/ResultSheet'
import { languageName, t } from '../../strings'
import ExerciseView from './ExerciseView'
import Summary from './Summary'
import { EMPTY_DRAFT, answerOf, type Draft } from './draft'

interface Props {
  started: LessonStarted
  /** The course language's code. */
  lang: string
  /** Back to the path, after the summary or on quitting. */
  onExit: () => void
}

/** The correct answer of an exercise when it is in the course language, to be heard on the sheet. */
function answerInTarget(exercise: Exercise): string | null {
  switch (exercise.type) {
    case 'translate_to_target':
      return exercise.target
    case 'fill_gap':
      return exercise.full_sentence
    case 'listen_type':
      return exercise.text
    default:
      return null
  }
}

/**
 * The lesson player, for lessons and reviews: one exercise at a time, its result sheet, then the
 * summary. Keyboard: Enter checks an answer and continues from the result sheet, keys 1 to 4
 * choose an option of a pick-the-meaning exercise, Esc asks to quit.
 *
 * A free translation the offline grader cannot decide goes to the AI judge when a provider is set
 * up: the sheet says "Checking…" meanwhile. An unchecked answer offers Check with AI, an answer
 * that was almost right or wrong offers Explain; either opens the AI setup dialog when AI is not
 * set up yet, and runs again after its Continue.
 *
 * Listening (module 09): every exercise shown asks for the next one's sentence to be prefetched;
 * with Play prompts aloud on, a prompt in the course language plays once when shown, and the
 * correct answer once on the result sheet when it is in the course language. Can't listen now
 * skips this and every later listening exercise of the lesson, with no grade.
 *
 * Speaking (module 10): a speaking exercise is answered with a take; Check sends it to be
 * transcribed and scored, and the result sheet shows its grade. Can't speak now skips this and
 * every later speaking exercise of the lesson with no grade, and so does a lesson opened while
 * speaking exercises are off, skipped for the hour, or without a microphone.
 */
export default function Lesson({ started, lang, onExit }: Props) {
  const exercises = started.exercises
  const [index, setIndex] = useState(0)
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT)
  const [grade, setGrade] = useState<Grade | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [quitting, setQuitting] = useState(false)
  const [summary, setSummary] = useState<LessonSummary | null>(null)
  const [wordBank, setWordBank] = useState(false)
  // Play prompts aloud (audio.autoplay), read when the lesson opens.
  const [autoplay, setAutoplay] = useState(false)
  // Can't listen now was chosen: listening exercises are skipped for the rest of the lesson.
  const [listenOff, setListenOff] = useState(false)
  // Speaking exercises are skipped for the rest of the lesson (Can't speak now, speaking off, no
  // microphone).
  const [speakOff, setSpeakOff] = useState(false)
  // The take of the speaking exercise on screen, its checked result, and what went wrong.
  const [take, setTake] = useState<Recording | null>(null)
  const [speakResult, setSpeakResult] = useState<SpeakResult | null>(null)
  const [speakError, setSpeakError] = useState<AppError | null>(null)
  // `speaking.pass_threshold`, for the score line.
  const [passMark, setPassMark] = useState(0.8)
  // The exercise shown, the skips and the grade, for the callbacks of commands that answer later.
  const latest = useRef({ index: 0, listenOff: false, graded: false })
  // The AI judge is deciding the answer (a translation's submission, or Check with AI).
  const [checking, setChecking] = useState(false)
  const [explain, setExplain] = useState<ExplainView | null>(null)
  const [setup, setSetup] = useState(false)
  // The AI action that opened the setup dialog, run again after its Continue.
  const afterSetup = useRef<(() => void) | null>(null)
  // The exercise on screen, so a late explanation never lands on the next one.
  const onScreen = useRef<string | null>(null)
  const [claimState, setClaimState] = useState<string | null>(null)
  const shownAt = useRef(0)
  const quitTitle = useId()

  const exercise = exercises[index]

  useEffect(() => {
    shownAt.current = Date.now()
    onScreen.current = exercises[index]?.id ?? null
  }, [index, exercises])

  useEffect(() => {
    latest.current = { index, listenOff, graded: grade !== null || speakResult !== null }
  })

  // The status arrives after every AI call; the 402 link's label follows its claim state.
  useEffect(() => {
    let cancelled = false
    const unlisten = onAiStatus((status) => {
      if (!cancelled) setClaimState(status.claim_state)
    })
    return () => {
      cancelled = true
      void unlisten.then((stop) => stop())
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    const speaking = exercises.some((shown) => shown.type === 'speak')
    settingsGet().then(
      (settings) => {
        if (cancelled) return
        if (settings?.answer_input === 'word_bank') setWordBank(true)
        if (settings?.audio?.autoplay) setAutoplay(true)
        if (typeof settings?.speaking?.pass_threshold === 'number') {
          setPassMark(settings.speaking.pass_threshold)
        }
        if (speaking && settings?.speaking?.enabled === false) speakingOff()
      },
      () => {},
    )
    // Only a lesson with speaking exercises asks whether they can be done now.
    if (speaking) {
      speakingStatus().then(
        (status) => {
          if (cancelled || !status) return
          const skipped = status.speak_off_until !== null && status.speak_off_until > Date.now()
          if (skipped || !status.microphone) speakingOff()
        },
        () => {},
      )
    }
    return () => {
      cancelled = true
    }
    // The lesson's exercises never change while it runs; speakingOff reads the latest state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Each exercise shown asks for the next one's sentence to be fetched ahead; a failure is
  // nobody's business but the cache's.
  useEffect(() => {
    const shown = exercises[index]
    if (!shown || !started.session_id) return
    ttsPrefetch(started.session_id, shown.id).catch(() => {})
  }, [index, exercises, started.session_id])

  async function check() {
    if (busy || grade || !exercise) return
    if (exercise.type === 'speak') {
      await checkSpeech(false)
      return
    }
    const answer = answerOf(exercise, draft, wordBank)
    if (!answer) {
      setError(
        exercise.type === 'pick_meaning'
          ? t('exercise.empty_choice')
          : exercise.type === 'match_pairs'
            ? t('exercise.empty_pairs')
            : t('exercise.empty_text'),
      )
      return
    }
    setBusy(true)
    setError(null)
    // Only a free translation can go to the AI judge, which takes a few seconds.
    setChecking(exercise.type === 'translate_to_target' || exercise.type === 'translate_to_en')
    try {
      const result = await exerciseSubmit(
        started.session_id,
        exercise.id,
        answer,
        Math.max(0, Date.now() - shownAt.current),
        false,
      )
      if (result) setGrade(result)
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
      setChecking(false)
    }
  }

  /**
   * Check on a speaking exercise: the take is transcribed (by the provider, or by the local model
   * with `local`) and scored, and the grade shows on the result sheet. No consent yet opens the
   * AI setup dialog and checks again after its Continue.
   */
  async function checkSpeech(local: boolean) {
    if (busy || !exercise || exercise.type !== 'speak') return
    if (!take) {
      setSpeakError({ code: '', message: t('speak.record_first'), action: null })
      return
    }
    const exerciseId = exercise.id
    setBusy(true)
    setChecking(true)
    setSpeakError(null)
    try {
      const result = await speakCheck(started.session_id, exerciseId, take.id, local)
      if (onScreen.current !== exerciseId || !result) return
      setSpeakResult(result)
      setGrade(result.grade)
    } catch (reason) {
      if (onScreen.current !== exerciseId) return
      const failed = asAppError(reason) ?? { code: '', message: errorMessage(reason), action: null }
      if (failed.code === 'needs_consent') {
        askForSetup(() => void checkSpeech(local))
      } else {
        setSpeakError(failed)
      }
    } finally {
      setBusy(false)
      setChecking(false)
    }
  }

  /** Try again after a result: the take and the result go, and a new take is checked again. */
  function trySpeakingAgain() {
    setTake(null)
    setSpeakResult(null)
    setSpeakError(null)
    setGrade(null)
  }

  /**
   * Speaking exercises are off for the rest of the lesson; the one on screen, unanswered, is
   * skipped at once. Called when the lesson learns it from the settings or the speaking status.
   */
  function speakingOff() {
    setSpeakOff(true)
    const { index: current, listenOff: listening, graded } = latest.current
    if (exercises[current]?.type === 'speak' && !graded) goTo(after(current, listening, true))
  }

  /** Can't speak now: off for an hour, and this lesson goes on without its speaking exercises. */
  async function cantSpeak() {
    if (busy || speakResult) return
    try {
      await sessionSkip('speak')
    } catch {
      // The lesson skips them all the same.
    }
    setSpeakOff(true)
    goTo(after(index, listenOff, true))
  }

  /** Opens the AI setup dialog; its Continue runs `action`, Not now leaves things as they are. */
  function askForSetup(action: () => void) {
    afterSetup.current = action
    setSetup(true)
  }

  /** Check with AI, and Try again after the judge's call failed. */
  async function checkWithAi() {
    if (!exercise) return
    const exerciseId = exercise.id
    setBusy(true)
    setChecking(true)
    setError(null)
    try {
      const result = await exerciseCheck(started.session_id, exerciseId)
      if (result) setGrade(result)
    } catch (reason) {
      if (asAppError(reason)?.code === 'needs_consent') {
        askForSetup(() => void checkWithAi())
      } else {
        setError(errorMessage(reason))
      }
    } finally {
      setBusy(false)
      setChecking(false)
    }
  }

  async function explainThis() {
    if (!exercise) return
    const exerciseId = exercise.id
    setExplain({ state: 'loading' })
    try {
      const result = await explainAnswer(started.session_id, exerciseId)
      if (onScreen.current !== exerciseId) return
      setExplain(result ? { state: 'done', text: result.text, tip: result.tip ?? null } : null)
    } catch (reason) {
      if (onScreen.current !== exerciseId) return
      const appError = asAppError(reason)
      if (appError?.code === 'needs_consent') {
        setExplain(null)
        askForSetup(() => void explainThis())
      } else {
        setExplain({
          state: 'error',
          error: appError ?? { code: '', message: errorMessage(reason), action: null },
        })
      }
    }
  }

  async function iWasRight() {
    if (busy || !exercise) return
    setBusy(true)
    try {
      const result = await exerciseIWasRight(started.session_id, exercise.id)
      if (result) setGrade(result)
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  async function finish() {
    setBusy(true)
    setError(null)
    try {
      const result = await lessonFinish(started.session_id)
      if (result) setSummary(result)
      else onExit()
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  /** The exercise after `from`, past the listening and speaking ones while they are skipped. */
  function after(from: number, skipListening: boolean, skipSpeaking: boolean): number {
    let to = from + 1
    while (
      to < exercises.length &&
      ((skipListening && exercises[to].type === 'listen_type') ||
        (skipSpeaking && exercises[to].type === 'speak'))
    ) {
      to += 1
    }
    return to
  }

  /** Shows exercise `to`, or finishes when there is none. */
  function goTo(to: number) {
    if (to < exercises.length) {
      onScreen.current = null
      setIndex(to)
      setDraft(EMPTY_DRAFT)
      setGrade(null)
      setError(null)
      setExplain(null)
      setTake(null)
      setSpeakResult(null)
      setSpeakError(null)
    } else {
      void finish()
    }
  }

  function next() {
    if (busy) return
    goTo(after(index, listenOff, speakOff))
  }

  /** Can't listen now: off for an hour, and this lesson goes on without its listening exercises. */
  async function cantListen() {
    if (busy || grade) return
    try {
      await sessionSkip('listen')
    } catch {
      // The lesson skips them all the same.
    }
    setListenOff(true)
    goTo(after(index, true, speakOff))
  }

  // The keyboard, while an exercise is on screen. Enter is taken from any focused element, and
  // its default action prevented, so a focused button is never pressed twice.
  useEffect(() => {
    if (summary) return
    function onKey(event: KeyboardEvent) {
      // The AI setup dialog takes the keyboard while it is open.
      if (setup) return
      if (quitting) {
        if (event.key === 'Escape') {
          event.preventDefault()
          setQuitting(false)
        }
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        setQuitting(true)
        return
      }
      if (busy || !exercise) return
      const target = event.target
      const typing =
        target instanceof HTMLTextAreaElement ||
        (target instanceof HTMLInputElement && target.type === 'text')
      if (event.key === 'Enter') {
        if (grade) {
          event.preventDefault()
          next()
        } else if (exercise.type === 'speak') {
          if (take) {
            event.preventDefault()
            void checkSpeech(false)
          }
        } else if (answerOf(exercise, draft, wordBank)) {
          event.preventDefault()
          void check()
        }
        return
      }
      if (
        !grade &&
        !typing &&
        exercise.type === 'pick_meaning' &&
        /^[1-4]$/.test(event.key) &&
        !event.ctrlKey &&
        !event.metaKey &&
        !event.altKey
      ) {
        const choice = Number(event.key) - 1
        if (choice < exercise.options.length) {
          event.preventDefault()
          setDraft({ ...draft, choice })
        }
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  if (summary) {
    return <Summary summary={summary} onContinue={onExit} />
  }

  const author = exercise
    ? started.attributions?.find((a) => a.exercise_id === exercise.id)?.author
    : undefined

  return (
    <main className="lesson">
      {exercise && (
        <p className="counter">{t('exercise.counter', { i: index + 1, n: exercises.length })}</p>
      )}
      {index === 0 && started.notice && <p className="notice">{started.notice}</p>}
      {index === 0 && started.ai_error && <AiBanner error={started.ai_error} />}
      {exercise ? (
        <ExerciseView
          key={exercise.id}
          exercise={exercise}
          lang={lang}
          language={languageName(lang)}
          wordBank={wordBank}
          draft={draft}
          checked={grade !== null}
          onDraft={setDraft}
          sessionId={started.session_id || null}
          autoPlay={autoplay}
          onCantListen={() => void cantListen()}
          speak={{
            take,
            onTake: setTake,
            result: speakResult,
            passMark,
            error: speakError,
            onError: setSpeakError,
            busy,
            keyboard: !quitting && !setup && !summary,
            claimState,
            onUseLocal: () => void checkSpeech(true),
            onRetry: () => void checkSpeech(false),
            onTryAgain: trySpeakingAgain,
            onCantSpeak: () => void cantSpeak(),
            onStatus: (status) => setClaimState(status.claim_state),
          }}
        />
      ) : null}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {checking || grade ? (
        // One sheet throughout: "Checking…" turns into the verdict in place.
        <ResultSheet
          grade={grade}
          checking={checking}
          exerciseType={exercise?.type}
          author={author}
          busy={busy}
          claimState={claimState}
          explain={explain}
          onIWasRight={() => void iWasRight()}
          onContinue={next}
          onCheckWithAi={() => void checkWithAi()}
          onExplain={exercise?.type === 'speak' ? undefined : () => void explainThis()}
          onStatus={(status) => setClaimState(status.claim_state)}
          answerPlay={
            exercise && grade && answerInTarget(exercise) ? (
              <PlayButton
                key={`answer-${exercise.id}`}
                text={answerInTarget(exercise) ?? ''}
                lang={lang}
                sessionId={started.session_id || null}
                autoPlay={autoplay}
              />
            ) : undefined
          }
        />
      ) : exercise ? (
        <div className="actions">
          <button type="button" className="primary" onClick={() => void check()} disabled={busy}>
            {t('exercise.check')}
          </button>
        </div>
      ) : (
        <div className="actions">
          <button type="button" className="primary" onClick={() => void finish()} disabled={busy}>
            {t('result.continue')}
          </button>
        </div>
      )}
      <p className="keys">{t('exercise.keys')}</p>
      {setup && (
        <AiSetupDialog
          onStatus={(status) => setClaimState(status.claim_state)}
          onClose={(proceed) => {
            setSetup(false)
            const action = afterSetup.current
            afterSetup.current = null
            if (proceed && action) action()
          }}
          onChooseOther={() => {
            // The answer stays as it is; the provider is chosen in Settings > AI after the lesson.
            setSetup(false)
            afterSetup.current = null
          }}
        />
      )}
      {quitting && (
        <div className="dialog-backdrop">
          <div role="dialog" aria-modal="true" aria-labelledby={quitTitle} className="dialog">
            <h2 id={quitTitle}>{t('exercise.quit_title')}</h2>
            <p>{t('exercise.quit_text')}</p>
            <div className="actions">
              <button type="button" onClick={onExit}>
                {t('exercise.quit')}
              </button>
              <button type="button" className="primary" autoFocus onClick={() => setQuitting(false)}>
                {t('exercise.keep_going')}
              </button>
            </div>
          </div>
        </div>
      )}
    </main>
  )
}
