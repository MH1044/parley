import type { Grade, Verdict } from '../api/commands'
import { t, type StringKey } from '../strings'

const VERDICT_KEYS: Record<Verdict, StringKey> = {
  correct: 'result.correct',
  almost: 'result.almost',
  typo: 'result.typo',
  wrong: 'result.wrong',
  unchecked: 'result.unchecked',
}

interface Props {
  grade: Grade
  /** The Tatoeba author of the exercise's sentence: a name, '' for none, undefined when the
   * sentence is not from Tatoeba. */
  author?: string
  busy?: boolean
  onIWasRight: () => void
  onContinue: () => void
}

/** The verdict of a checked answer, read out as it appears. */
export default function ResultSheet({ grade, author, busy, onIWasRight, onContinue }: Props) {
  return (
    <section
      aria-label={t('result.region')}
      aria-live="polite"
      className={`result-sheet verdict-${grade.verdict}`}
    >
      <p className="verdict">{t(VERDICT_KEYS[grade.verdict] ?? 'result.wrong')}</p>
      {grade.message && <p className="grade-message">{grade.message}</p>}
      {grade.corrected && (
        <p className="corrected">{t('result.corrected', { corrected: grade.corrected })}</p>
      )}
      {author !== undefined && (
        <p className="attribution">
          {author
            ? t('result.attribution', { author })
            : t('result.attribution_anonymous')}
        </p>
      )}
      <div className="actions">
        {grade.verdict === 'unchecked' && (
          <button type="button" onClick={onIWasRight} disabled={busy}>
            {t('result.i_was_right')}
          </button>
        )}
        <button type="button" className="primary" onClick={onContinue} disabled={busy}>
          {t('result.continue')}
        </button>
      </div>
    </section>
  )
}
