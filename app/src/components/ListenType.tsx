import { useId } from 'react'
import { t } from '../strings'
import PlayButton from './PlayButton'

interface Props {
  /** The sentence to hear; never shown before the answer is checked. */
  text: string
  /** The course language's code. */
  lang: string
  sessionId: string | null
  /** Play the sentence once when the exercise is shown. */
  autoPlay: boolean
  value: string
  /** The answer was checked: the box and Can't listen now are disabled. */
  checked: boolean
  onChange: (value: string) => void
  /** Can't listen now: this and every later listening exercise of the lesson are skipped. */
  onCantListen: () => void
}

/**
 * Type what you hear: Play and Slow, the answer box, and Can't listen now. The sentence itself is
 * not on screen; the result sheet shows it after the check.
 */
export default function ListenType({
  text,
  lang,
  sessionId,
  autoPlay,
  value,
  checked,
  onChange,
  onCantListen,
}: Props) {
  const id = useId()
  return (
    <div className="exercise listen-type">
      <h1>{t('exercise.listen_type')}</h1>
      <PlayButton text={text} lang={lang} sessionId={sessionId} autoPlay={autoPlay} />
      <div className="answer">
        <label htmlFor={id}>{t('exercise.answer')}</label>
        <input
          id={id}
          type="text"
          lang={lang}
          value={value}
          disabled={checked}
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          autoFocus
          onChange={(event) => onChange(event.target.value)}
        />
      </div>
      <div className="actions">
        <button type="button" className="link" disabled={checked} onClick={onCantListen}>
          {t('listen.cant_listen')}
        </button>
      </div>
    </div>
  )
}
