import { useId, useState } from 'react'
import {
  errorMessage,
  placementAnswer,
  type PlacementOutcome,
  type PlacementQuestion,
} from '../../api/commands'
import { t } from '../../strings'

interface Props {
  first: PlacementQuestion
  /** The course language's code, for the `lang` attributes. */
  lang: string
  /** The course language's English name. */
  language: string
  onDone: () => void
}

/**
 * The placement test: one typed translation at a time, into English or into the course language,
 * then where the learner starts.
 */
export default function Placement({ first, lang, language, onDone }: Props) {
  const [question, setQuestion] = useState<PlacementQuestion>(first)
  const [text, setText] = useState('')
  const [outcome, setOutcome] = useState<PlacementOutcome | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const answerId = useId()

  async function answer(given: string) {
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      const step = await placementAnswer(question.session_id, { text: given })
      if (step?.next) {
        setQuestion(step.next)
        setText('')
      } else if (step?.result) {
        setOutcome(step.result)
      }
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  if (outcome) {
    const where = { n: outcome.start_unit, title: outcome.start_unit_title }
    return (
      <main className="onboarding placement">
        <h1>{t('placement.title')}</h1>
        <p className="placement-result">
          {outcome.label === 'Pre-A1'
            ? t('placement.result_pre_a1', where)
            : t('placement.result', { label: outcome.label, ...where })}
        </p>
        <div className="actions">
          <button type="button" className="primary" autoFocus onClick={onDone}>
            {t('placement.go')}
          </button>
        </div>
      </main>
    )
  }

  const exercise = question.exercise
  // A skill the test comes back to may ask a word again, translated into the course language.
  const intoTarget = exercise.type === 'translate_to_target'
  const prompt =
    exercise.type === 'translate_to_target'
      ? exercise.prompt_en
      : exercise.type === 'translate_to_en'
        ? exercise.prompt_target
        : ''
  return (
    <main className="onboarding placement">
      <h1>{t('placement.title')}</h1>
      <p className="counter">{t('placement.question', { n: question.n })}</p>
      <h2>{intoTarget ? t('exercise.translate_to_target', { language }) : t('exercise.translate_to_en')}</h2>
      <p className="prompt" lang={intoTarget ? 'en' : lang}>
        {prompt}
      </p>
      <div className="answer">
        <label htmlFor={answerId}>{t('exercise.answer')}</label>
        <input
          id={answerId}
          type="text"
          lang={intoTarget ? lang : 'en'}
          value={text}
          autoComplete="off"
          spellCheck={false}
          autoFocus
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              void answer(text)
            }
          }}
        />
      </div>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="button" className="primary" disabled={busy} onClick={() => void answer(text)}>
          {t('placement.next')}
        </button>
        <button type="button" disabled={busy} onClick={() => void answer('')}>
          {t('placement.dont_know')}
        </button>
      </div>
    </main>
  )
}
