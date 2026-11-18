import type { LessonSummary } from '../../api/commands'
import StreakBadge from '../../components/StreakBadge'
import { t } from '../../strings'

interface Props {
  summary: LessonSummary
  onContinue: () => void
}

/**
 * What a finished lesson or review earned. The correct answers are counted over the checked ones;
 * an answer left Not checked is counted apart, on its own line, never as a wrong one.
 */
export default function Summary({ summary, onContinue }: Props) {
  return (
    <main className="summary">
      <h1>{t('summary.title')}</h1>
      <p className="xp">{t('summary.xp', { xp: summary.xp })}</p>
      <p>{t('summary.correct', { correct: summary.correct, checked: summary.checked })}</p>
      {summary.unchecked > 0 && (
        <p className="unchecked">{t('summary.unchecked', { unchecked: summary.unchecked })}</p>
      )}
      <StreakBadge streak={summary.streak} freezes={summary.freezes} />
      {summary.message && <p className="record">{summary.message}</p>}
      <div className="actions">
        <button type="button" className="primary" autoFocus onClick={onContinue}>
          {t('summary.continue')}
        </button>
      </div>
    </main>
  )
}
