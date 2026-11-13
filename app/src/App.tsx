import { useEffect, useState } from 'react'
import { appStart, asAppError, errorMessage } from './api/commands'
import {
  back,
  begin,
  dialogOpen,
  end,
  forward,
  historyButton,
  historyKey,
  isSecondary,
  open,
  outside,
  type Navigation,
} from './navigation'
import DataError from './screens/DataError/DataError'
import Lesson from './screens/Lesson/Lesson'
import Onboarding from './screens/Onboarding/Onboarding'
import Path from './screens/Path/Path'
import Review from './screens/Review/Review'
import Settings from './screens/Settings/Settings'
import Stats from './screens/Stats/Stats'
import Story from './screens/Story/Story'
import { t } from './strings'

export default function App() {
  const [navigation, setNavigation] = useState<Navigation>(() => outside({ name: 'loading' }))
  const [notice, setNotice] = useState<string | null>(null)
  // Bumped to run app_start again: after a restore, after deleting everything, or on retry.
  const [launches, setLaunches] = useState(0)
  const screen = navigation.screen

  useEffect(() => {
    let cancelled = false
    appStart().then(
      (launch) => {
        if (cancelled) return
        setNotice(launch?.notice ?? null)
        if (launch?.onboarded && launch.active_course) {
          setNavigation(open(outside({ name: 'loading' }), { name: 'path', lang: launch.active_course }))
        } else {
          setNavigation(outside({ name: 'onboarding' }))
        }
      },
      (reason: unknown) => {
        if (cancelled) return
        const error = asAppError(reason)
        if (error?.code === 'storage') {
          const details = error.action?.details
          setNavigation(outside({ name: 'data_error', details: typeof details === 'string' ? details : '' }))
        } else {
          setNavigation(outside({ name: 'failed', message: errorMessage(reason) }))
        }
      },
    )
    return () => {
      cancelled = true
    }
  }, [launches])

  // Leaving a screen: Esc on a screen the path opened returns to the path; the mouse's back and
  // forward buttons and Alt+Left and Alt+Right walk the history. The web view's own history
  // navigation, which those buttons and keys would start, is always cancelled, so the page is
  // never left or reloaded. A lesson takes Esc itself (it asks to quit), a dialog takes Esc first
  // (it closes), and nothing leaves a screen while a dialog is open.
  useEffect(() => {
    function step(direction: 'back' | 'forward') {
      if (dialogOpen()) return
      setNavigation((current) => (direction === 'back' ? back(current) : forward(current)))
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        if (event.defaultPrevented || dialogOpen() || !isSecondary(screen)) return
        event.preventDefault()
        const lang = screen.lang
        setNavigation((current) => open(current, { name: 'path', lang }))
        return
      }
      const direction = historyKey(event)
      if (!direction) return
      event.preventDefault()
      step(direction)
    }
    // A side button arrives as mousedown, mouseup and auxclick: all three are cancelled, and the
    // move happens once, on mouseup.
    function onMouse(event: MouseEvent) {
      const direction = historyButton(event)
      if (!direction) return
      event.preventDefault()
      if (event.type === 'mouseup') step(direction)
    }
    window.addEventListener('keydown', onKey)
    window.addEventListener('mousedown', onMouse)
    window.addEventListener('mouseup', onMouse)
    window.addEventListener('auxclick', onMouse)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('mousedown', onMouse)
      window.removeEventListener('mouseup', onMouse)
      window.removeEventListener('auxclick', onMouse)
    }
  }, [screen])

  const restart = () => {
    setNotice(null)
    setNavigation(outside({ name: 'loading' }))
    setLaunches((n) => n + 1)
  }

  const toPath = (lang: string) => () => setNavigation((current) => open(current, { name: 'path', lang }))
  const endActivity = (lang: string) => () => setNavigation((current) => end(current, lang))
  // A new step of the history is a new screen, even when it shows the same screen as another.
  const step = navigation.at

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
      body = <Onboarding onDone={(lang) => setNavigation((current) => open(current, { name: 'path', lang }))} />
      break
    case 'path': {
      const lang = screen.lang
      body = (
        <Path
          key={`${step}:${lang}`}
          lang={lang}
          onStart={(started) =>
            setNavigation((current) =>
              begin(
                current,
                started.kind === 'story'
                  ? { name: 'story', lang, started }
                  : { name: 'lesson', lang, started },
              ),
            )
          }
          onNothingToReview={(text) =>
            setNavigation((current) => open(current, { name: 'review_empty', lang, notice: text }))
          }
          onStats={() => setNavigation((current) => open(current, { name: 'stats', lang }))}
          onSettings={(tab) => setNavigation((current) => open(current, { name: 'settings', lang, tab }))}
        />
      )
      break
    }
    case 'lesson':
      body = (
        <Lesson
          key={screen.started.session_id}
          started={screen.started}
          lang={screen.lang}
          onExit={endActivity(screen.lang)}
        />
      )
      break
    case 'story':
      body = <Story key={screen.started.session_id} started={screen.started} onExit={endActivity(screen.lang)} />
      break
    case 'review_empty':
      body = <Review key={step} notice={screen.notice} onBack={toPath(screen.lang)} />
      break
    case 'stats':
      body = <Stats key={step} lang={screen.lang} onBack={toPath(screen.lang)} />
      break
    case 'settings':
      body = <Settings key={step} initialTab={screen.tab} onBack={toPath(screen.lang)} onRestart={restart} />
      break
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
