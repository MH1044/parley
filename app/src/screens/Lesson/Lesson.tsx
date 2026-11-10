import { useEffect, useId, useRef, useState } from 'react'
import {
  errorMessage,
  exerciseIWasRight,
  exerciseSubmit,
  lessonFinish,
  settingsGet,
  type Grade,
  type LessonStarted,
  type LessonSummary,
} from '../../api/commands'
import AiBanner from '../../components/AiBanner'
import ResultSheet from '../../components/ResultSheet'
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

/**
 * The lesson player, for lessons and reviews: one exercise at a time, its result sheet, then the
 * summary. Keyboard: Enter checks an answer and continues from the result sheet, keys 1 to 4
 * choose an option of a pick-the-meaning exercise, Esc asks to quit.
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
  const shownAt = useRef(0)
  const quitTitle = useId()

  const exercise = exercises[index]

  useEffect(() => {
    shownAt.current = Date.now()
  }, [index])

  useEffect(() => {
    let cancelled = false
    settingsGet().then(
      (settings) => {
        if (!cancelled && settings?.answer_input === 'word_bank') setWordBank(true)
      },
      () => {},
    )
    return () => {
      cancelled = true
    }
  }, [])

  async function check() {
    if (busy || grade || !exercise) return
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

  function next() {
    if (busy) return
    if (index + 1 < exercises.length) {
      setIndex(index + 1)
      setDraft(EMPTY_DRAFT)
      setGrade(null)
      setError(null)
    } else {
      void finish()
    }
  }

  // The keyboard, while an exercise is on screen. Enter is taken from any focused element, and
  // its default action prevented, so a focused button is never pressed twice.
  useEffect(() => {
    if (summary) return
    function onKey(event: KeyboardEvent) {
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
        />
      ) : null}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {grade ? (
        <ResultSheet
          grade={grade}
          author={author}
          busy={busy}
          onIWasRight={() => void iWasRight()}
          onContinue={next}
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
