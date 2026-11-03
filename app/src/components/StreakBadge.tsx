import { t } from '../strings'

interface Props {
  streak: number
  freezes?: number
}

/** The streak in days and, when there are any, the freezes banked. */
export default function StreakBadge({ streak, freezes = 0 }: Props) {
  return (
    <div className="streak-badge">
      <span className="streak-days">{t('streak.days', { n: streak })}</span>
      {freezes > 0 && <span className="streak-freezes">{t('streak.freezes', { n: freezes })}</span>}
    </div>
  )
}
