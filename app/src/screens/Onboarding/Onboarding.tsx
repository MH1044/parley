import { useState } from 'react'
import {
  courseList,
  courseStart,
  errorMessage,
  onPackProgress,
  packDownload,
  packStatus,
  placementStart,
  settingsSet,
  type CourseSummary,
  type PackProgress,
  type PackStatus,
  type PlacementQuestion,
} from '../../api/commands'
import { languageName, t } from '../../strings'
import Placement from './Placement'

type Step = 'welcome' | 'language' | 'pack' | 'goal' | 'start' | 'placement'

const GOALS = [10, 20, 30, 50]
const DEFAULT_GOAL = 20

interface Props {
  /** Onboarding is done: open the path of this course. */
  onDone: (lang: string) => void
}

/**
 * The first run, one step at a time: welcome, the language, the course pack, the daily goal and
 * where to start (the placement test or the beginning). No dialog appears here: AI consent is
 * asked at the first AI action, never during onboarding.
 */
export default function Onboarding({ onDone }: Props) {
  const [step, setStep] = useState<Step>('welcome')
  const [courses, setCourses] = useState<CourseSummary[] | null>(null)
  const [course, setCourse] = useState<CourseSummary | null>(null)
  const [pack, setPack] = useState<PackStatus | null>(null)
  const [packError, setPackError] = useState<string | null>(null)
  const [progress, setProgress] = useState<PackProgress | null>(null)
  const [downloading, setDownloading] = useState(false)
  const [goal, setGoal] = useState(DEFAULT_GOAL)
  const [question, setQuestion] = useState<PlacementQuestion | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const lang = course?.lang ?? ''
  const language = course?.name || languageName(lang)

  async function attempt(action: () => Promise<void>) {
    setBusy(true)
    setError(null)
    try {
      await action()
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  const start = () => {
    setStep('language')
    void attempt(async () => {
      const list = await courseList()
      setCourses(list ?? [])
    })
  }

  const choose = (chosen: CourseSummary) =>
    attempt(async () => {
      const started = await courseStart(chosen.lang)
      setCourse(started ?? chosen)
      setPack(null)
      setPackError(null)
      try {
        setPack((await packStatus(chosen.lang)) ?? null)
      } catch (reason) {
        setPackError(errorMessage(reason))
      }
      setStep('pack')
    })

  const download = async () => {
    setDownloading(true)
    setPackError(null)
    setProgress(null)
    const unlisten = await onPackProgress((p) => setProgress(p))
    try {
      const status = await packDownload(lang)
      if (status) setPack(status)
    } catch (reason) {
      setPackError(errorMessage(reason))
    } finally {
      unlisten()
      setDownloading(false)
    }
  }

  const saveGoal = () =>
    attempt(async () => {
      await settingsSet({ daily_goal_xp: goal })
      setStep('start')
    })

  const takePlacement = () =>
    attempt(async () => {
      const first = await placementStart(lang)
      if (first) {
        setQuestion(first)
        setStep('placement')
      }
    })

  const errorLine = error && (
    <p role="alert" className="error">
      {error}
    </p>
  )

  switch (step) {
    case 'welcome':
      return (
        <main className="onboarding">
          <h1>{t('onboarding.welcome.title')}</h1>
          <p>{t('onboarding.welcome.text')}</p>
          <div className="actions">
            <button type="button" className="primary" autoFocus onClick={start}>
              {t('onboarding.welcome.start')}
            </button>
          </div>
        </main>
      )
    case 'language':
      return (
        <main className="onboarding">
          <h1>{t('onboarding.language.title')}</h1>
          <p>{t('onboarding.language.text')}</p>
          <div className="actions languages">
            {(courses ?? []).map((c) => (
              <button key={c.lang} type="button" disabled={busy} onClick={() => void choose(c)}>
                {c.name}
              </button>
            ))}
          </div>
          {errorLine}
        </main>
      )
    case 'pack': {
      const next = (
        <button type="button" className="primary" onClick={() => setStep('goal')}>
          {t('onboarding.continue')}
        </button>
      )
      let body
      if (downloading) {
        body = (
          <>
            <p>{t('onboarding.pack.downloading', { language })}</p>
            <progress
              aria-label={t('onboarding.pack.progress')}
              max={progress?.bytes_total || 1}
              value={progress?.bytes_done ?? 0}
            />
          </>
        )
      } else if (packError) {
        body = (
          <>
            <p role="alert" className="error">
              {packError}
            </p>
            <div className="actions">{next}</div>
          </>
        )
      } else if (pack?.state === 'available') {
        body = (
          <>
            <p>{t('onboarding.pack.available', { language, size: pack.size ?? '' })}</p>
            <div className="actions">
              <button type="button" className="primary" onClick={() => void download()}>
                {t('onboarding.pack.download')}
              </button>
              <button type="button" onClick={() => setStep('goal')}>
                {t('onboarding.pack.skip')}
              </button>
            </div>
          </>
        )
      } else if (pack?.state === 'installed') {
        body = (
          <>
            <p>{t('onboarding.pack.installed', { language })}</p>
            <div className="actions">{next}</div>
          </>
        )
      } else {
        body = (
          <>
            <p>{t('onboarding.pack.not_published', { language })}</p>
            <div className="actions">{next}</div>
          </>
        )
      }
      return (
        <main className="onboarding">
          <h1>{t('onboarding.pack.title')}</h1>
          {body}
        </main>
      )
    }
    case 'goal':
      return (
        <main className="onboarding">
          <h1>{t('onboarding.goal.title')}</h1>
          <p>{t('onboarding.goal.text')}</p>
          <div role="radiogroup" aria-label={t('onboarding.goal.title')} className="options">
            {GOALS.map((xp) => (
              <label key={xp} className="option">
                <input
                  type="radio"
                  name="daily-goal"
                  value={xp}
                  checked={goal === xp}
                  onChange={() => setGoal(xp)}
                />
                <span>{t('goal.option', { xp })}</span>
              </label>
            ))}
          </div>
          <div className="actions">
            <button type="button" className="primary" disabled={busy} onClick={() => void saveGoal()}>
              {t('onboarding.continue')}
            </button>
          </div>
          {errorLine}
        </main>
      )
    case 'start':
      return (
        <main className="onboarding">
          <h1>{t('onboarding.start.title')}</h1>
          <div className="actions column">
            <button
              type="button"
              className="primary"
              disabled={busy}
              onClick={() => void takePlacement()}
            >
              {t('onboarding.start.placement')}
            </button>
            <button type="button" disabled={busy} onClick={() => onDone(lang)}>
              {t('onboarding.start.beginning')}
            </button>
          </div>
          {errorLine}
        </main>
      )
    case 'placement':
      return question ? (
        <Placement first={question} lang={lang} onDone={() => onDone(lang)} />
      ) : null
  }
}
