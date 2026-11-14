import { useEffect, useId, useState } from 'react'
import {
  errorMessage,
  onModelProgress,
  settingsGet,
  settingsSet,
  speakingStatus,
  whisperDownload,
  whisperModels,
  type ModelInfo,
  type ModelProgress,
  type Settings,
  type SpeakingStatus,
} from '../../api/commands'
import { t } from '../../strings'

/** The pass marks offered: 0.5 to 1.0 in steps of 0.05. */
const PASS_MIN = 0.5
const PASS_MAX = 1.0
const PASS_STEP = 0.05

/**
 * Settings > Speaking (docs/modules/10-speaking-and-pronunciation.md, UI): Speaking exercises,
 * Check my speaking with AI provider or Local model (the local model only in a build that has
 * it), the local models with their sizes, Download and Use, the pass mark, Keep my recordings,
 * and the measured cost of a speaking check.
 */
export default function SpeakingSection() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [status, setStatus] = useState<SpeakingStatus | null>(null)
  const [models, setModels] = useState<ModelInfo[] | null>(null)
  const [downloading, setDownloading] = useState<string | null>(null)
  const [progress, setProgress] = useState<ModelProgress | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [failure, setFailure] = useState<string | null>(null)
  const transcriberName = useId()
  const modelName = useId()
  const passId = useId()

  useEffect(() => {
    let cancelled = false
    settingsGet().then(
      (found) => {
        if (!cancelled && found) setSettings(found)
      },
      (reason: unknown) => {
        if (!cancelled) setFailure(errorMessage(reason))
      },
    )
    speakingStatus().then(
      (found) => {
        if (cancelled || !found) return
        setStatus(found)
        // The model list is for a build that can use it.
        if (found.local_available) {
          whisperModels().then(
            (list) => {
              if (!cancelled && list) setModels(list)
            },
            () => {},
          )
        }
      },
      () => {},
    )
    const unlisten = onModelProgress((update) => {
      if (!cancelled) setProgress(update)
    })
    return () => {
      cancelled = true
      void unlisten.then((stop) => stop())
    }
  }, [])

  async function change(patch: Record<string, unknown>) {
    setFailure(null)
    try {
      const saved = await settingsSet({ speaking: patch })
      if (saved) setSettings(saved)
      setMessage(t('settings.saved'))
    } catch (reason) {
      setFailure(errorMessage(reason))
    }
  }

  async function download(name: string) {
    setDownloading(name)
    setProgress(null)
    setFailure(null)
    try {
      const list = await whisperDownload(name)
      if (list) setModels(list)
    } catch (reason) {
      setFailure(errorMessage(reason))
    } finally {
      setDownloading(null)
      setProgress(null)
    }
  }

  if (!settings) {
    return failure ? <p role="alert">{failure}</p> : null
  }

  const speaking = settings.speaking
  const localAvailable = status?.local_available === true
  const chosenModel = speaking.whisper_model ?? 'base'
  const percent = (update: ModelProgress | null) =>
    update && update.bytes_total > 0
      ? Math.floor((update.bytes_done / update.bytes_total) * 100)
      : 0

  return (
    <section className="speaking-settings" aria-label={t('speaking.tab')}>
      <label className="switch">
        <input
          type="checkbox"
          role="switch"
          checked={speaking.enabled}
          onChange={(event) => void change({ enabled: event.target.checked })}
        />
        {t('speaking.enabled')}
      </label>

      <fieldset className="transcriber">
        <legend>{t('speaking.transcriber')}</legend>
        <label className="option">
          <input
            type="radio"
            name={transcriberName}
            value="provider"
            checked={speaking.transcriber === 'provider'}
            onChange={() => void change({ transcriber: 'provider' })}
          />
          {t('speaking.provider')}
        </label>
        {localAvailable ? (
          <label className="option">
            <input
              type="radio"
              name={transcriberName}
              value="local"
              checked={speaking.transcriber === 'local'}
              onChange={() => void change({ transcriber: 'local' })}
            />
            {t('speaking.local')}
          </label>
        ) : (
          status && <p className="hint">{t('speaking.local_unavailable')}</p>
        )}
      </fieldset>

      {localAvailable && models && (
        <fieldset className="speech-models">
          <legend>{t('speaking.models')}</legend>
          {models.map((model) => (
            <div key={model.name} className="model">
              <span className="model-name">
                {t('speaking.model', { name: model.name, size: model.size })}
              </span>
              {model.downloaded ? (
                <span className="downloaded">{t('speaking.downloaded')}</span>
              ) : downloading === model.name ? (
                <span role="status">
                  {t('speaking.downloading', {
                    name: model.name,
                    percent: percent(progress?.name === model.name ? progress : null),
                  })}
                </span>
              ) : (
                <button
                  type="button"
                  disabled={downloading !== null}
                  onClick={() => void download(model.name)}
                >
                  {t('speaking.download', { size: model.size })}
                </button>
              )}
              <label className="option">
                <input
                  type="radio"
                  name={modelName}
                  value={model.name}
                  checked={chosenModel === model.name}
                  onChange={() => void change({ whisper_model: model.name })}
                />
                {t('speaking.use_model', { name: model.name })}
              </label>
            </div>
          ))}
        </fieldset>
      )}

      <div className="field">
        <label htmlFor={passId}>{t('speaking.pass_mark')}</label>
        <input
          id={passId}
          type="range"
          min={PASS_MIN}
          max={PASS_MAX}
          step={PASS_STEP}
          value={speaking.pass_threshold}
          onChange={(event) => {
            // Whole steps only, so 0.8 is saved as 0.8 and not 0.8000000001.
            const steps = Math.round((Number(event.target.value) - PASS_MIN) / PASS_STEP)
            const mark = Math.min(PASS_MAX, PASS_MIN + steps * PASS_STEP)
            void change({ pass_threshold: Number(mark.toFixed(2)) })
          }}
        />
        <span className="value">
          {t('speaking.pass_mark_value', { percent: Math.round(speaking.pass_threshold * 100) })}
        </span>
      </div>

      <label className="switch">
        <input
          type="checkbox"
          role="switch"
          checked={speaking.keep_recordings}
          onChange={(event) => void change({ keep_recordings: event.target.checked })}
        />
        {t('speaking.keep')}
      </label>
      <p className="hint">{t('speaking.keep_hint')}</p>

      {status && <p className="cost-line">{status.cost_line}</p>}
      {message && (
        <p role="status" className="status">
          {message}
        </p>
      )}
      {failure && (
        <p role="alert" className="error">
          {failure}
        </p>
      )}
    </section>
  )
}
