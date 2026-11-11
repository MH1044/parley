import type { AiStatus, AppError, Exercise, Grade, Verdict } from '../api/commands'
import { t, type StringKey } from '../strings'
import AiBanner from './AiBanner'

const VERDICT_KEYS: Record<Verdict, StringKey> = {
  correct: 'result.correct',
  almost: 'result.almost',
  typo: 'result.typo',
  wrong: 'result.wrong',
  unchecked: 'result.unchecked',
}

/** What Explain shows: the explanation being written, written, or why it could not be. */
export type ExplainView =
  | { state: 'loading' }
  | { state: 'done'; text: string; tip: string | null }
  | { state: 'error'; error: AppError }

interface Props {
  /** The grade to show; none yet while the AI judge decides the first time. */
  grade: Grade | null
  /** The AI judge is deciding: the sheet says "Checking…" and offers nothing. */
  checking?: boolean
  /** The exercise's type: Explain is never offered on a match-the-pairs answer. */
  exerciseType?: Exercise['type']
  /** The Tatoeba author of the exercise's sentence: a name, '' for none, undefined when the
   * sentence is not from Tatoeba. */
  author?: string
  busy?: boolean
  /** The AI status's claim state, for the label of a 402's one link. */
  claimState?: string | null
  /** What Explain shows, once asked for. */
  explain?: ExplainView | null
  onIWasRight: () => void
  onContinue: () => void
  /** Check with AI on an unchecked answer, and Try again after the judge's call failed. */
  onCheckWithAi?: () => void
  /** Explain, after an answer that was almost right or wrong. */
  onExplain?: () => void
  /** Reconnect succeeded: the new status. */
  onStatus?: (status: AiStatus) => void
}

/**
 * The verdict of a checked answer, read out as it appears: the verdict line, the reason, the
 * correct answer, and the ways on. An unchecked answer offers Check with AI (or, when the AI judge's
 * call failed, says why, as the AI banner does) and I was right; an answer that was almost right or
 * wrong offers Explain. While the AI judge decides, the same sheet says "Checking…" with no buttons.
 */
export default function ResultSheet({
  grade,
  checking,
  exerciseType,
  author,
  busy,
  claimState,
  explain,
  onIWasRight,
  onContinue,
  onCheckWithAi,
  onExplain,
  onStatus,
}: Props) {
  if (checking || !grade) {
    return (
      <section aria-label={t('result.region')} aria-live="polite" className="result-sheet checking">
        <p className="verdict" role="status">
          {t('result.checking')}
        </p>
      </section>
    )
  }
  const unchecked = grade.verdict === 'unchecked'
  const judgeError = unchecked ? (grade.judge_error ?? null) : null
  const explainable =
    (grade.verdict === 'almost' || grade.verdict === 'wrong') &&
    exerciseType !== 'match_pairs' &&
    onExplain !== undefined

  return (
    <section
      aria-label={t('result.region')}
      aria-live="polite"
      className={`result-sheet verdict-${grade.verdict}`}
    >
      <p className="verdict">{t(VERDICT_KEYS[grade.verdict] ?? 'result.wrong')}</p>
      {judgeError && (
        <AiBanner
          error={judgeError}
          claimState={claimState}
          onRetry={onCheckWithAi}
          onStatus={onStatus}
        />
      )}
      {grade.message && <p className="grade-message">{grade.message}</p>}
      {!unchecked && grade.explanation && <p className="judge-explanation">{grade.explanation}</p>}
      {grade.corrected && (
        <p className="corrected">{t('result.corrected', { corrected: grade.corrected })}</p>
      )}
      {grade.expected && (
        <p className="also-correct">{t('result.also_correct', { expected: grade.expected })}</p>
      )}
      {author !== undefined && (
        <p className="attribution">
          {author
            ? t('result.attribution', { author })
            : t('result.attribution_anonymous')}
        </p>
      )}
      {explain?.state === 'loading' && <p role="status">{t('result.explaining')}</p>}
      {explain?.state === 'done' && (
        <div className="explanation">
          <p>{explain.text}</p>
          {explain.tip && <p className="tip">{t('result.tip', { tip: explain.tip })}</p>}
        </div>
      )}
      {explain?.state === 'error' && (
        <AiBanner
          error={explain.error}
          claimState={claimState}
          onRetry={onExplain}
          onStatus={onStatus}
        />
      )}
      <div className="actions">
        {unchecked && !judgeError && onCheckWithAi && (
          <button type="button" onClick={onCheckWithAi} disabled={busy}>
            {t('result.check_with_ai')}
          </button>
        )}
        {unchecked && (
          <button type="button" onClick={onIWasRight} disabled={busy}>
            {t('result.i_was_right')}
          </button>
        )}
        {explainable && !explain && (
          <button type="button" onClick={onExplain} disabled={busy}>
            {t('result.explain')}
          </button>
        )}
        <button type="button" className="primary" onClick={onContinue} disabled={busy}>
          {t('result.continue')}
        </button>
      </div>
    </section>
  )
}
