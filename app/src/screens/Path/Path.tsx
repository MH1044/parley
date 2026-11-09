import { useEffect, useState } from 'react'
import {
  aiStatus,
  errorMessage,
  lessonStart,
  onAiStatus,
  pathGet,
  reviewStart,
  skillGet,
  type AiStatus,
  type LessonStarted,
  type PathView,
  type SkillDetail as Detail,
} from '../../api/commands'
import AiChip from '../../components/AiChip'
import AiSetupDialog from '../../components/AiSetupDialog'
import StreakBadge from '../../components/StreakBadge'
import type { SettingsTab } from '../Settings/Settings'
import { languageName, t } from '../../strings'
import { stateLabel } from './labels'
import SkillDetail from './SkillDetail'

/** How many cards a review takes at most. */
const REVIEW_LIMIT = 20

interface Props {
  lang: string
  /** A lesson or a review with exercises has started. */
  onStart: (started: LessonStarted) => void
  /** A review found nothing due: its notice. */
  onNothingToReview: (notice: string) => void
  onStats: () => void
  /** Opens Settings, on its AI part when the learner chose their own key or a local model. */
  onSettings: (tab?: SettingsTab) => void
}

/** The learning path: units and skills in order, the streak, today's XP, and the way in. */
export default function Path({ lang, onStart, onNothingToReview, onStats, onSettings }: Props) {
  const [path, setPath] = useState<PathView | null>(null)
  const [detail, setDetail] = useState<Detail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  // Nothing AI-related shows until ai_status answers, and nothing while it fails.
  const [ai, setAi] = useState<AiStatus | null>(null)
  const [setup, setSetup] = useState(false)

  useEffect(() => {
    let cancelled = false
    aiStatus(false).then(
      (status) => {
        if (!cancelled && status) setAi(status)
      },
      () => {},
    )
    const unlisten = onAiStatus((status) => {
      if (!cancelled) setAi(status)
    })
    return () => {
      cancelled = true
      void unlisten.then((stop) => stop())
    }
  }, [])

  useEffect(() => {
    let cancelled = false
    pathGet(lang).then(
      (view) => {
        if (!cancelled && view) setPath(view)
      },
      (reason: unknown) => {
        if (!cancelled) setError(errorMessage(reason))
      },
    )
    return () => {
      cancelled = true
    }
  }, [lang])

  async function startLesson(skillId: string) {
    setBusy(true)
    setError(null)
    try {
      const started = await lessonStart(skillId, 'learn')
      if (started) onStart(started)
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  async function startReview() {
    setBusy(true)
    setError(null)
    try {
      const started = await reviewStart(lang, REVIEW_LIMIT)
      if (!started) return
      if (started.exercises.length > 0) onStart(started)
      else onNothingToReview(started.notice ?? '')
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  async function openSkill(skillId: string) {
    setError(null)
    try {
      const found = await skillGet(skillId)
      if (found) setDetail(found)
    } catch (reason) {
      setError(errorMessage(reason))
    }
  }

  if (detail) {
    return (
      <SkillDetail
        detail={detail}
        busy={busy}
        error={error}
        onStart={() => void startLesson(detail.id)}
        onBack={() => {
          setDetail(null)
          setError(null)
        }}
      />
    )
  }

  return (
    <main className="path">
      <header className="path-header">
        <h1>{languageName(lang)}</h1>
        {path && (
          <>
            <StreakBadge streak={path.streak} freezes={path.freezes} />
            <p className="today">{t('path.today', { xp: path.today_xp, goal: path.goal_xp })}</p>
          </>
        )}
        <nav className="actions">
          {path?.next_skill_id && (
            <button
              type="button"
              className="primary"
              disabled={busy}
              onClick={() => path.next_skill_id && void startLesson(path.next_skill_id)}
            >
              {t('path.continue')}
            </button>
          )}
          <button type="button" disabled={busy} onClick={() => void startReview()}>
            {t('path.review', { due: path?.due ?? 0 })}
          </button>
          <button type="button" onClick={onStats}>
            {t('path.stats')}
          </button>
          <button type="button" onClick={() => onSettings()}>
            {t('path.settings')}
          </button>
        </nav>
        {ai && ai.provider !== 'unset' && <AiChip status={ai} />}
      </header>
      {ai?.provider === 'unset' && (
        <section className="ai-card">
          <p>{t('ai.card.text')}</p>
          <button type="button" onClick={() => setSetup(true)}>
            {t('ai.card.set_up')}
          </button>
        </section>
      )}
      {setup && (
        <AiSetupDialog
          onStatus={setAi}
          onClose={() => setSetup(false)}
          onChooseOther={() => {
            setSetup(false)
            onSettings('ai')
          }}
        />
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {path && !path.next_skill_id && <p>{t('path.all_done')}</p>}
      {path?.units.map((unit) => (
        <section key={unit.id} className="unit">
          <h2>{t('path.unit', { n: unit.n, title: unit.title })}</h2>
          <ul className="skills">
            {unit.skills.map((skill) => (
              <li key={skill.id} className={`skill state-${skill.state}`}>
                <button type="button" onClick={() => void openSkill(skill.id)}>
                  {skill.title}
                </button>
                <span className="state">{stateLabel(skill.state)}</span>
                {skill.tested_out && <span className="tested-out">{t('path.tested_out')}</span>}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </main>
  )
}
