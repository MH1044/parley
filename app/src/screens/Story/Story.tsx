import { useState } from 'react'
import {
  errorMessage,
  exerciseSubmit,
  lessonFinish,
  type Grade,
  type LessonStarted,
  type LessonSummary,
} from '../../api/commands'
import { t } from '../../strings'
import Summary from '../Lesson/Summary'

interface Props {
  started: LessonStarted
  onExit: () => void
}

/**
 * A graded story (module 07): the title and its English translation, the paragraphs, then each
 * question with its options as buttons. An answer shows Correct or the right option; after the
 * last, Finish shows the XP. Stories never change review cards.
 */
export default function Story({ started, onExit }: Props) {
  const story = started.story
  const [grades, setGrades] = useState<Record<number, Grade>>({})
  const [summary, setSummary] = useState<LessonSummary | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  if (!story) return null
  if (summary) return <Summary summary={summary} onContinue={onExit} />

  const done = story.questions.every((_, i) => grades[i] !== undefined)

  async function answer(question: number, choice: number) {
    setBusy(true)
    setError(null)
    try {
      const grade = await exerciseSubmit(started.session_id, `q${question + 1}`, { choice }, null, false)
      if (grade) setGrades((all) => ({ ...all, [question]: grade }))
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
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="story">
      <h1 lang={started.skill_id?.slice(0, 2)}>{story.title}</h1>
      <p className="title-en">{story.title_en}</p>
      {story.paragraphs.map((paragraph, i) => (
        <p key={i} className="paragraph" lang={started.skill_id?.slice(0, 2)}>
          {paragraph}
        </p>
      ))}
      <ol className="questions">
        {story.questions.map((question, i) => {
          const grade = grades[i]
          return (
            <li key={i}>
              <p>{question.question_en}</p>
              <div className="options">
                {question.options.map((option, j) => (
                  <button
                    key={j}
                    type="button"
                    disabled={busy || grade !== undefined}
                    onClick={() => void answer(i, j)}
                  >
                    {option}
                  </button>
                ))}
              </div>
              {grade && (
                <p role="status" className={`verdict ${grade.verdict}`}>
                  {grade.verdict === 'correct'
                    ? t('story.correct')
                    : t('story.wrong', { answer: grade.corrected ?? '' })}
                </p>
              )}
            </li>
          )
        })}
      </ol>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <div className="actions">
        {done ? (
          <button type="button" className="primary" disabled={busy} onClick={() => void finish()}>
            {t('story.finish')}
          </button>
        ) : (
          <button type="button" onClick={onExit}>
            {t('story.back')}
          </button>
        )}
      </div>
    </main>
  )
}
