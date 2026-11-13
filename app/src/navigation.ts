import { useEffect } from 'react'
import type { LessonStarted } from './api/commands'
import type { SettingsTab } from './screens/Settings/Settings'

/** Which screen shows; the app routes by state, with no router. */
export type Screen =
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

/** A screen the history keeps: the path and the screens it opens. */
export type Visit = Extract<Screen, { name: 'path' | 'review_empty' | 'stats' | 'settings' }>

/** A screen the path opens and Back to my path or Esc leaves. */
export type Secondary = Exclude<Visit, { name: 'path' }>

/** A lesson or a story: left only through its own controls, never by back or forward. */
export type Activity = Extract<Screen, { name: 'lesson' | 'story' }>

/**
 * The screen showing and the screens visited, oldest first, with the one the learner is at.
 * During a lesson or a story `at` stays on the path it was started from.
 */
export interface Navigation {
  screen: Screen
  visits: Visit[]
  at: number
}

export function isVisit(screen: Screen): screen is Visit {
  return (
    screen.name === 'path' ||
    screen.name === 'review_empty' ||
    screen.name === 'stats' ||
    screen.name === 'settings'
  )
}

export function isSecondary(screen: Screen): screen is Secondary {
  return isVisit(screen) && screen.name !== 'path'
}

/** A screen outside the history (loading, an error, onboarding): the history starts again. */
export function outside(screen: Exclude<Screen, Visit | Activity>): Navigation {
  return { screen, visits: [], at: -1 }
}

/** Opens a screen as the next step of the history; whatever was forward of here is dropped. */
export function open(navigation: Navigation, visit: Visit): Navigation {
  const visits = [...navigation.visits.slice(0, navigation.at + 1), visit]
  return { screen: visit, visits, at: visits.length - 1 }
}

/** Starts a lesson or a story from the path: not a step of its own, but forward is dropped. */
export function begin(navigation: Navigation, activity: Activity): Navigation {
  return { screen: activity, visits: navigation.visits.slice(0, navigation.at + 1), at: navigation.at }
}

/** A lesson or story ended: back on the path it was started from. */
export function end(navigation: Navigation, lang: string): Navigation {
  const from = navigation.visits[navigation.at]
  if (from?.name === 'path' && from.lang === lang) return { ...navigation, screen: from }
  return open(navigation, { name: 'path', lang })
}

/** The previous screen of the history; nothing on the first one, in a lesson or outside it. */
export function back(navigation: Navigation): Navigation {
  if (!isVisit(navigation.screen) || navigation.at <= 0) return navigation
  const at = navigation.at - 1
  return { ...navigation, screen: navigation.visits[at], at }
}

/** The next screen of the history; nothing on the last one, in a lesson or outside it. */
export function forward(navigation: Navigation): Navigation {
  if (!isVisit(navigation.screen) || navigation.at >= navigation.visits.length - 1) return navigation
  const at = navigation.at + 1
  return { ...navigation, screen: navigation.visits[at], at }
}

/** Whether a dialog is open: then Esc, back and forward never leave the screen. */
export function dialogOpen(): boolean {
  return document.querySelector('[role="dialog"]') !== null
}

/**
 * Esc for an open dialog: `onEscape` runs and the key is taken, so the screen under the dialog
 * does not see it. It listens on the document, which hears a key before the window, where the
 * app and the lesson player listen. `null` while the dialog is closed.
 */
export function useEscape(onEscape: (() => void) | null) {
  useEffect(() => {
    if (!onEscape) return
    const run = onEscape
    function onKey(event: KeyboardEvent) {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      event.preventDefault()
      run()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onEscape])
}

/** A field that takes Alt+Left and Alt+Right itself (moving by word on macOS). */
function editable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable || target instanceof HTMLTextAreaElement) return true
  return target instanceof HTMLInputElement && !['button', 'checkbox', 'radio', 'submit', 'reset'].includes(target.type)
}

/** Alt+Left is back and Alt+Right forward, except in a text field. */
export function historyKey(event: KeyboardEvent): 'back' | 'forward' | null {
  if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || editable(event.target)) return null
  if (event.key === 'ArrowLeft') return 'back'
  if (event.key === 'ArrowRight') return 'forward'
  return null
}

/** The mouse's side buttons: 3 is back and 4 forward. */
export function historyButton(event: MouseEvent): 'back' | 'forward' | null {
  if (event.button === 3) return 'back'
  if (event.button === 4) return 'forward'
  return null
}
