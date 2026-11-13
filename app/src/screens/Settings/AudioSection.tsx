import { useEffect, useId, useState } from 'react'
import {
  aiStatus,
  audioStatus,
  cacheClear,
  errorMessage,
  settingsGet,
  settingsSet,
  voicesList,
  type Settings,
  type Voice,
} from '../../api/commands'
import AiSetupDialog from '../../components/AiSetupDialog'
import { languageName, t, type StringKey } from '../../strings'

/** The course languages, each with its own OS voice. */
const LANGS = ['es', 'fr', 'de'] as const
type Lang = (typeof LANGS)[number]

/** The slow speeds offered: 0.5 to 1.0 in steps of 0.05. */
const SLOW_MIN = 0.5
const SLOW_MAX = 1.0
const SLOW_STEP = 0.05

/** The help of a missing voice on this computer's platform, as the core words it. */
function noVoiceHelp(language: string): string {
  const platform = typeof navigator === 'undefined' ? '' : navigator.userAgent
  const key: StringKey = /Windows/i.test(platform)
    ? 'audio.no_voice_windows'
    : /Mac/i.test(platform)
      ? 'audio.no_voice_macos'
      : 'audio.no_voice_linux'
  return t(key, { language })
}

/** One language's OS voice picker, or "No voice installed" and how to add one. */
function VoicePicker(props: {
  lang: Lang
  voices: Voice[] | undefined
  chosen: string | null
  onChoose: (id: string) => void
}) {
  const id = useId()
  const language = languageName(props.lang)
  if (props.voices === undefined) return null
  const os = props.voices.filter((voice) => voice.source === 'os')
  if (os.length === 0) {
    return (
      <div className="field">
        <span className="label">{t('audio.settings.language_voice', { language })}</span>
        <p>{t('audio.settings.no_voice')}</p>
        <p className="hint">{noVoiceHelp(language)}</p>
      </div>
    )
  }
  const value = os.some((voice) => voice.id === props.chosen) ? (props.chosen ?? '') : os[0].id
  return (
    <div className="field">
      <label htmlFor={id}>{t('audio.settings.language_voice', { language })}</label>
      <select id={id} value={value} onChange={(event) => props.onChoose(event.target.value)}>
        {os.map((voice) => (
          <option key={voice.id} value={voice.id}>
            {voice.name}
          </option>
        ))}
      </select>
    </div>
  )
}

/**
 * Settings > Audio: System voice or Natural voice (turning natural voices on before AI is set up
 * opens the AI setup dialog; Not now keeps the system voice), the OS voice of each language, the
 * slow speed, Play prompts aloud, Clear audio cache, and the measured cost of natural voices.
 */
export default function AudioSection() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [voices, setVoices] = useState<Partial<Record<Lang, Voice[]>>>({})
  const [costLine, setCostLine] = useState<string | null>(null)
  const [status, setStatus] = useState<string | null>(null)
  const [dialog, setDialog] = useState(false)
  const [busy, setBusy] = useState(false)
  const sourceName = useId()
  const slowId = useId()

  useEffect(() => {
    let cancelled = false
    settingsGet().then(
      (found) => {
        if (!cancelled && found) setSettings(found)
      },
      (reason: unknown) => {
        if (!cancelled) setStatus(errorMessage(reason))
      },
    )
    audioStatus().then(
      (found) => {
        if (!cancelled && found) setCostLine(found.cost_line)
      },
      () => {},
    )
    for (const lang of LANGS) {
      voicesList(lang).then(
        (found) => {
          if (!cancelled) setVoices((all) => ({ ...all, [lang]: found ?? [] }))
        },
        () => {},
      )
    }
    return () => {
      cancelled = true
    }
  }, [])

  async function change(patch: Record<string, unknown>) {
    try {
      const saved = await settingsSet(patch)
      if (saved) setSettings(saved)
      setStatus(t('settings.saved'))
    } catch (reason) {
      setStatus(errorMessage(reason))
    }
  }

  /** Natural voices need a provider and consent first (I12); the dialog asks for both. */
  async function chooseSource(source: 'os' | 'natural') {
    if (source === 'os') {
      await change({ audio: { voice_source: 'os' } })
      return
    }
    if (!settings || settings.ai.provider === 'unset') {
      setDialog(true)
      return
    }
    let consented = true
    try {
      const ai = await aiStatus(false)
      if (ai && (ai.provider === 'unset' || ai.needs_consent)) consented = false
    } catch {
      // The status could not be read; the provider is chosen, and a play asks again if need be.
    }
    if (!consented) {
      setDialog(true)
      return
    }
    await change({ audio: { voice_source: 'natural' } })
  }

  async function clearCache() {
    setBusy(true)
    try {
      const cleared = await cacheClear('audio')
      setStatus(t('audio.settings.cleared', { n: cleared?.files_removed ?? 0 }))
    } catch (reason) {
      setStatus(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  if (!settings) {
    return status ? <p role="alert">{status}</p> : null
  }

  const source = settings.audio.voice_source
  const slow = settings.audio.slow_speed

  return (
    <section className="audio-settings" aria-label={t('settings.audio')}>
      <fieldset className="voice-source">
        <legend>{t('audio.settings.source')}</legend>
        <label className="option">
          <input
            type="radio"
            name={sourceName}
            value="os"
            checked={source === 'os'}
            onChange={() => void chooseSource('os')}
          />
          {t('audio.settings.system_voice')}
        </label>
        <label className="option">
          <input
            type="radio"
            name={sourceName}
            value="natural"
            checked={source === 'natural'}
            onChange={() => void chooseSource('natural')}
          />
          {t('audio.settings.natural_voice')}
        </label>
        {costLine && <p className="cost-line">{costLine}</p>}
      </fieldset>

      <fieldset className="voices">
        <legend>{t('audio.settings.voices')}</legend>
        {LANGS.map((lang) => (
          <VoicePicker
            key={lang}
            lang={lang}
            voices={voices[lang]}
            chosen={settings.audio.os_voice[lang]}
            onChoose={(id) => void change({ audio: { os_voice: { [lang]: id } } })}
          />
        ))}
      </fieldset>

      <div className="field">
        <label htmlFor={slowId}>{t('audio.settings.slow_speed')}</label>
        <input
          id={slowId}
          type="range"
          min={SLOW_MIN}
          max={SLOW_MAX}
          step={SLOW_STEP}
          value={slow}
          onChange={(event) => {
            // Whole steps only, so 0.75 is saved as 0.75 and not 0.7500000001.
            const steps = Math.round((Number(event.target.value) - SLOW_MIN) / SLOW_STEP)
            const speed = Math.min(SLOW_MAX, SLOW_MIN + steps * SLOW_STEP)
            void change({ audio: { slow_speed: Number(speed.toFixed(2)) } })
          }}
        />
        <span className="value">{t('audio.settings.slow_speed_value', { speed: slow.toFixed(2) })}</span>
      </div>

      <label className="switch">
        <input
          type="checkbox"
          role="switch"
          checked={settings.audio.autoplay}
          onChange={(event) => void change({ audio: { autoplay: event.target.checked } })}
        />
        {t('audio.settings.autoplay')}
      </label>

      <div className="actions">
        <button type="button" disabled={busy} onClick={() => void clearCache()}>
          {t('audio.settings.clear_cache')}
        </button>
      </div>

      {status && (
        <p role="status" className="status">
          {status}
        </p>
      )}
      {dialog && (
        <AiSetupDialog
          onClose={(proceed) => {
            setDialog(false)
            // Continue after the consent: natural voices are on; Not now keeps the system voice.
            if (proceed) void change({ audio: { voice_source: 'natural' } })
          }}
          onChooseOther={() => setDialog(false)}
        />
      )}
    </section>
  )
}
