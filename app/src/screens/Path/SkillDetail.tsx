import type { SkillDetail as Detail } from '../../api/commands'
import { t } from '../../strings'
import { stateLabel } from './labels'

interface Props {
  detail: Detail
  busy: boolean
  error: string | null
  onStart: () => void
  onBack: () => void
}

/** A skill: what it teaches, its words and state, and the way into its next lesson. */
export default function SkillDetail({ detail, busy, error, onStart, onBack }: Props) {
  const locked = detail.state === 'locked'
  return (
    <main className="skill-detail">
      <p className="unit-title">{detail.unit_title}</p>
      <h1>{detail.title}</h1>
      <p className="can-do">{detail.can_do}</p>
      <p className="state">
        <span>{stateLabel(detail.state)}</span>{' '}
        <span>{t('skill.mastery', { percent: Math.round(detail.mastery * 100) })}</span>
      </p>
      {detail.grammar_note && (
        <section>
          <h2>{t('skill.grammar')}</h2>
          <p>{detail.grammar_note}</p>
        </section>
      )}
      {detail.words.length > 0 && (
        <section>
          <h2>{t('skill.words')}</h2>
          <ul className="words">
            {detail.words.map((word) => (
              <li key={`${word.target}-${word.en}`}>
                <span className="target">{word.target}</span>{' '}
                <span className="en" lang="en">
                  {word.en}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <div className="actions">
        {locked ? (
          detail.opened_by && <p>{t('skill.locked', { opened_by: detail.opened_by })}</p>
        ) : (
          <button type="button" className="primary" disabled={busy} onClick={onStart}>
            {t('skill.start')}
          </button>
        )}
        <button type="button" onClick={onBack}>
          {t('app.back_to_path')}
        </button>
      </div>
    </main>
  )
}
