import { useEffect, useState } from 'react'
import { appStart, asAppError, errorMessage, type LessonStarted } from './api/commands'
import DataError from './screens/DataError/DataError'
import Lesson from './screens/Lesson/Lesson'
import Onboarding from './screens/Onboarding/Onboarding'
import Path from './screens/Path/Path'
import Review from './screens/Review/Review'
import Settings, { type SettingsTab } from './screens/Settings/Settings'
import Stats from './screens/Stats/Stats'
import Story from './screens/Story/Story'
import { t } from './strings'

/** Which screen shows; the app routes by state, with no router. */
type Screen =
  | { name: 'loading' }
  | { name: 'failed'; message: string }
  | { name: 'data_error'; details: string }
  | { name: 'onboarding' }
  | { name: 'path'; lang: string }
  | { name: 'lesson'; lang: string; started: LessonStarted }
  | { name: 'story'; lang: string; started: LessonStarted }
  | { name: 'review_empty'; lang: string; notice: string }
  | { name: 'stats'; lang: string }
  | { name: 'settings'; lang: string; tab?: SettingsTab }

export default function App() {
  const [screen, setScreen] = useState<Screen>({ name: 'loading' })
  const [notice, setNotice] = useState<string | null>(null)
  // Bumped to run app_start again: after a restore, after deleting everything, or on retry.
  const [launches, setLaunches] = useState(0)

  useEffect(() => {
    let cancelled = false
    appStart().then(
      (launch) => {
        if (cancelled) return
        setNotice(launch?.notice ?? null)
        if (launch?.onboarded && launch.active_course) {
          setScreen({ name: 'path', lang: launch.active_course })
        } else {
          setScreen({ name: 'onboarding' })
        }
      },
      (reason: unknown) => {
        if (cancelled) return
        const error = asAppError(reason)
        if (error?.code === 'storage') {
          const details = error.action?.details
          setScreen({ name: 'data_error', details: typeof details === 'string' ? details : '' })
        } else {
          setScreen({ name: 'failed', message: errorMessage(reason) })
        }
      },
    )
    return () => {
      cancelled = true
    }
  }, [launches])

  const restart = () => {
    setNotice(null)
    setScreen({ name: 'loading' })
    setLaunches((n) => n + 1)
  }

  let body
  switch (screen.name) {
    case 'loading':
      body = (
        <main className="loading">
          <p>{t('app.loading')}</p>
        </main>
      )
      break
    case 'failed':
      body = (
        <main className="failed">
          <p role="alert">{t('app.error', { message: screen.message })}</p>
          <button type="button" onClick={restart}>
            {t('app.retry')}
          </button>
        </main>
      )
      break
    case 'data_error':
      body = <DataError details={screen.details} onRestart={restart} />
      break
    case 'onboarding':
      body = <Onboarding onDone={(lang) => setScreen({ name: 'path', lang })} />
      break
    case 'path': {
      const lang = screen.lang
      body = (
        <Path
          key={lang}
          lang={lang}
          onStart={(started) =>
            setScreen(
              started.kind === 'story'
                ? { name: 'story', lang, started }
                : { name: 'lesson', lang, started },
            )
          }
          onNothingToReview={(text) => setScreen({ name: 'review_empty', lang, notice: text })}
          onStats={() => setScreen({ name: 'stats', lang })}
          onSettings={(tab) => setScreen({ name: 'settings', lang, tab })}
        />
      )
      break
    }
    case 'lesson': {
      const lang = screen.lang
      body = (
        <Lesson
          key={screen.started.session_id}
          started={screen.started}
          lang={lang}
          onExit={() => setScreen({ name: 'path', lang })}
        />
      )
      break
    }
    case 'story': {
      const lang = screen.lang
      body = (
        <Story
          key={screen.started.session_id}
          started={screen.started}
          onExit={() => setScreen({ name: 'path', lang })}
        />
      )
      break
    }
    case 'review_empty': {
      const lang = screen.lang
      body = <Review notice={screen.notice} onBack={() => setScreen({ name: 'path', lang })} />
      break
    }
    case 'stats': {
      const lang = screen.lang
      body = <Stats lang={lang} onBack={() => setScreen({ name: 'path', lang })} />
      break
    }
    case 'settings': {
      const lang = screen.lang
      body = (
        <Settings
          initialTab={screen.tab}
          onBack={() => setScreen({ name: 'path', lang })}
          onRestart={restart}
        />
      )
      break
    }
  }

  return (
    <>
      {notice && (
        <div role="status" className="notice-banner">
          <p>{notice}</p>
          <button type="button" onClick={() => setNotice(null)}>
            {t('app.notice_dismiss')}
          </button>
        </div>
      )}
      {body}
    </>
  )
}
