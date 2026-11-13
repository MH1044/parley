import { useEffect, useState } from 'react'
import { errorMessage, statsGet, type Stats as StatsView } from '../../api/commands'
import StreakBadge from '../../components/StreakBadge'
import { t } from '../../strings'
import { stateLabel } from '../Path/labels'

interface Props {
  lang: string
  onBack: () => void
}

/** The streak, the last 30 days, weekly XP, words known, recall and skills. */
export default function Stats({ lang, onBack }: Props) {
  const [stats, setStats] = useState<StatsView | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    statsGet(lang).then(
      (found) => {
        if (!cancelled && found) setStats(found)
      },
      (reason: unknown) => {
        if (!cancelled) setError(errorMessage(reason))
      },
    )
    return () => {
      cancelled = true
    }
  }, [lang])

  // Back to my path at the top as well as the foot: on a full course the skills list is long.
  const backToPath = (
    <div className="actions">
      <button type="button" onClick={onBack}>
        {t('app.back_to_path')}
      </button>
    </div>
  )

  return (
    <main className="stats">
      <h1>{t('stats.title')}</h1>
      {backToPath}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {stats?.empty && <p>{t('stats.empty')}</p>}
      {stats && !stats.empty && (
        <>
          <StreakBadge streak={stats.streak} freezes={stats.freezes} />
          <section>
            <h2>{t('stats.calendar')}</h2>
            <ol className="calendar">
              {stats.days.map((day) => (
                <li
                  key={day.local_date}
                  className={day.frozen ? 'day frozen' : day.active ? 'day active' : 'day'}
                  title={
                    day.frozen
                      ? t('stats.day_frozen', { date: day.local_date })
                      : t('stats.day', { date: day.local_date, xp: day.xp })
                  }
                  aria-label={
                    day.frozen
                      ? t('stats.day_frozen', { date: day.local_date })
                      : t('stats.day', { date: day.local_date, xp: day.xp })
                  }
                />
              ))}
            </ol>
          </section>
          <p>{t('stats.week', { xp: stats.week_xp })}</p>
          <p>{t('stats.record', { xp: stats.record_week_xp })}</p>
          <p>{t('stats.words', { known: stats.words_known, total: stats.vocab_items })}</p>
          <p>
            {stats.recall_rate === null
              ? t('stats.recall_none')
              : t('stats.recall', {
                  percent: Math.round(stats.recall_rate * 100),
                  reviews: stats.reviews_30,
                })}
          </p>
          {stats.cefr_estimate && <p>{t('stats.level', { cefr: stats.cefr_estimate })}</p>}
          <section>
            <h2>{t('stats.skills')}</h2>
            <ul className="skill-progress">
              {stats.skills.map((skill) => (
                <li key={skill.id}>
                  <span>{skill.title}</span> <span>{stateLabel(skill.state)}</span>{' '}
                  <meter min={0} max={1} value={skill.mastery} aria-label={skill.title} />
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
      {backToPath}
    </main>
  )
}
