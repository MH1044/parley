import { t } from '../../strings'

interface Props {
  /** Why there is nothing to review, and when the next review is. */
  notice: string
  onBack: () => void
}

/** A review with nothing due: when the next one is, and the way back. */
export default function Review({ notice, onBack }: Props) {
  return (
    <main className="review-empty">
      <h1>{t('review.title')}</h1>
      <p className="notice">{notice}</p>
      <div className="actions">
        <button type="button" className="primary" autoFocus onClick={onBack}>
          {t('app.back_to_path')}
        </button>
      </div>
    </main>
  )
}
