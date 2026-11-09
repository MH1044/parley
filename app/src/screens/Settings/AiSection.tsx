import { useEffect, useId, useState } from 'react'
import {
  aiCheckConnection,
  aiClearOwnKey,
  aiConsentAccept,
  aiDisconnectPublik,
  aiLocalProbe,
  aiOpenLink,
  aiReconnectPublik,
  aiSetLocal,
  aiSetOwnKey,
  aiStatus,
  asAppError,
  onAiStatus,
  settingsGet,
  type AiLink,
  type AiStatus,
  type AppError,
  type LocalModel,
} from '../../api/commands'
import AiBanner from '../../components/AiBanner'
import AiSetupDialog from '../../components/AiSetupDialog'
import BalanceLine from '../../components/BalanceLine'
import { t } from '../../strings'

/** The default local server: Ollama's OpenAI-compatible endpoint. */
const DEFAULT_SERVER = 'http://localhost:11434/v1'

/** A model's size in decimal gigabytes (one decimal) or megabytes. */
function size(bytes: number): string {
  if (bytes >= 1_000_000_000) {
    return t('ai.settings.size_gb', { size: (bytes / 1_000_000_000).toFixed(1) })
  }
  return t('ai.settings.size_mb', { size: Math.max(1, Math.round(bytes / 1_000_000)) })
}

/** An AppError for anything a command rejected with. */
function toError(reason: unknown): AppError {
  return asAppError(reason) ?? { code: '', message: String(reason), action: null }
}

/** A text field with its label. */
function Field(props: {
  label: string
  value: string
  onChange: (value: string) => void
  type?: 'text' | 'password'
}) {
  const id = useId()
  return (
    <div className="field">
      <label htmlFor={id}>{props.label}</label>
      <input
        id={id}
        type={props.type ?? 'text'}
        autoComplete="off"
        spellCheck={false}
        value={props.value}
        onChange={(event) => props.onChange(event.target.value)}
      />
    </div>
  )
}

/**
 * Settings > AI: the three providers, always all three and in this order (docs/PUBLIK-INTEGRATION.md
 * §3), the chosen one marked In use, and Check connection. The key field is never filled back:
 * the interface never receives a key.
 */
export default function AiSection() {
  const [status, setStatus] = useState<AiStatus | null>(null)
  const [error, setError] = useState<AppError | null>(null)
  const [retry, setRetry] = useState<(() => void) | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [dialog, setDialog] = useState(false)

  const [baseUrl, setBaseUrl] = useState('')
  const [key, setKey] = useState('')
  const [chatModel, setChatModel] = useState('')
  const [transcribeModel, setTranscribeModel] = useState('')
  const [speechModel, setSpeechModel] = useState('')
  const [speechVoice, setSpeechVoice] = useState('')
  const [server, setServer] = useState(DEFAULT_SERVER)
  const [models, setModels] = useState<LocalModel[] | null>(null)

  const titles = {
    publik: useId(),
    ownKey: useId(),
    local: useId(),
  }

  useEffect(() => {
    let cancelled = false
    aiStatus(true).then(
      (found) => {
        if (!cancelled && found) setStatus(found)
      },
      (reason: unknown) => {
        if (!cancelled) setError(toError(reason))
      },
    )
    settingsGet().then(
      (settings) => {
        if (cancelled || !settings?.ai) return
        const own = settings.ai.own_key
        setBaseUrl(own.base_url ?? '')
        setChatModel(own.chat_model ?? '')
        setTranscribeModel(own.transcribe_model ?? '')
        setSpeechModel(own.speech_model ?? '')
        setSpeechVoice(own.speech_voice ?? '')
        setServer(settings.ai.local.base_url || DEFAULT_SERVER)
      },
      () => {},
    )
    const unlisten = onAiStatus((changed) => {
      if (!cancelled) setStatus(changed)
    })
    return () => {
      cancelled = true
      void unlisten.then((stop) => stop())
    }
  }, [])

  /** Runs one action; a failure shows its banner (or the setup dialog when consent is needed). */
  async function run(action: () => Promise<void>, again?: () => void) {
    setBusy(true)
    setError(null)
    setNote(null)
    setRetry(null)
    try {
      await action()
    } catch (reason) {
      const failure = toError(reason)
      if (failure.code === 'needs_consent') {
        setDialog(true)
      } else {
        setError(failure)
        if (again) setRetry(() => again)
      }
    } finally {
      setBusy(false)
    }
  }

  const keep = (next: AiStatus | null | undefined) => {
    if (next) setStatus(next)
  }

  const checkConnection = () =>
    run(
      async () => {
        const checked = await aiCheckConnection()
        if (checked) setNote(checked.message)
      },
      () => void checkConnection(),
    )

  const usePublik = () => {
    if (status?.needs_consent) {
      setDialog(true)
      return
    }
    void run(async () => keep(await aiConsentAccept()))
  }

  const openLink = (kind: AiLink) => run(async () => void (await aiOpenLink(kind)))

  const saveKey = () =>
    run(async () => {
      keep(
        await aiSetOwnKey({
          base_url: baseUrl,
          key,
          chat_model: chatModel,
          transcribe_model: transcribeModel.trim() || null,
          speech_model: speechModel.trim() || null,
          speech_voice: speechVoice.trim() || null,
        }),
      )
      setKey('')
      setNote(t('ai.settings.key_saved'))
    })

  const removeKey = () =>
    run(async () => {
      keep(await aiClearOwnKey())
      setNote(t('ai.settings.key_removed'))
    })

  const findModels = () =>
    run(async () => {
      setModels(null)
      const found = await aiLocalProbe(server)
      setModels(found ?? [])
    })

  const chooseModel = (name: string) => run(async () => keep(await aiSetLocal(server, name)))

  const provider = status?.provider ?? 'unset'
  const inUse = (which: string) =>
    provider === which ? <span className="in-use">{t('ai.settings.in_use')}</span> : null
  const claimed = status?.claim_state === 'claimed'

  return (
    <section className="ai-settings" aria-label={t('ai.settings.tab')}>
      <section className="provider" aria-labelledby={titles.publik}>
        <div className="provider-head">
          <h2 id={titles.publik}>{t('ai.settings.publik')}</h2>
          {inUse('publik')}
        </div>
        <BalanceLine line={status?.balance_line} />
        <div className="actions">
          {(provider === 'publik' || status?.claim_url || status?.add_credit_url) &&
            (claimed ? (
              <button type="button" disabled={busy} onClick={() => void openLink('add_credit')}>
                {t('ai.link.add_credit')}
              </button>
            ) : (
              <button type="button" disabled={busy} onClick={() => void openLink('claim')}>
                {t('ai.link.sign_in')}
              </button>
            ))}
          {provider !== 'publik' && (
            <button type="button" disabled={busy} onClick={usePublik}>
              {t('ai.settings.use_publik')}
            </button>
          )}
          {provider === 'publik' && status && !status.connected && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(async () => keep(await aiReconnectPublik()))}
            >
              {t('ai.settings.reconnect')}
            </button>
          )}
          {provider === 'publik' && status?.connected && !status.needs_consent && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run(async () => keep(await aiDisconnectPublik()))}
            >
              {t('ai.settings.disconnect')}
            </button>
          )}
        </div>
      </section>

      <section className="provider" aria-labelledby={titles.ownKey}>
        <div className="provider-head">
          <h2 id={titles.ownKey}>{t('ai.settings.own_key')}</h2>
          {inUse('own_key')}
        </div>
        <Field label={t('ai.settings.base_url')} value={baseUrl} onChange={setBaseUrl} />
        <Field label={t('ai.settings.key')} value={key} onChange={setKey} type="password" />
        {status?.own_key_stored && <p className="hint">{t('ai.settings.key_stored')}</p>}
        <Field label={t('ai.settings.chat_model')} value={chatModel} onChange={setChatModel} />
        <Field
          label={t('ai.settings.transcribe_model')}
          value={transcribeModel}
          onChange={setTranscribeModel}
        />
        <Field
          label={t('ai.settings.speech_model')}
          value={speechModel}
          onChange={setSpeechModel}
        />
        <Field
          label={t('ai.settings.speech_voice')}
          value={speechVoice}
          onChange={setSpeechVoice}
        />
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void saveKey()}>
            {t('ai.settings.save_key')}
          </button>
          {status?.own_key_stored && (
            <button type="button" disabled={busy} onClick={() => void removeKey()}>
              {t('ai.settings.remove_key')}
            </button>
          )}
        </div>
      </section>

      <section className="provider" aria-labelledby={titles.local}>
        <div className="provider-head">
          <h2 id={titles.local}>{t('ai.settings.local')}</h2>
          {inUse('local')}
        </div>
        <Field label={t('ai.settings.server')} value={server} onChange={setServer} />
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void findModels()}>
            {t('ai.settings.find_models')}
          </button>
        </div>
        {provider === 'local' && status?.local_model && (
          <p>{t('ai.settings.model_in_use', { model: status.local_model })}</p>
        )}
        {models && models.length === 0 && <p>{t('ai.settings.no_models')}</p>}
        {models && models.length > 0 && (
          <ul className="models" aria-label={t('ai.settings.models')}>
            {models.map((model) => (
              <li key={model.name}>
                <span className="model-name">{model.name}</span>
                {model.bytes !== null && <span className="model-size">{size(model.bytes)}</span>}
                <button type="button" disabled={busy} onClick={() => void chooseModel(model.name)}>
                  {t('ai.settings.use_model')}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <div className="actions">
        <button type="button" disabled={busy} onClick={() => void checkConnection()}>
          {t('ai.settings.check')}
        </button>
      </div>
      {note && (
        <p role="status" className="status">
          {note}
        </p>
      )}
      {error && (
        <AiBanner
          error={error}
          claimState={status?.claim_state}
          onRetry={retry ?? undefined}
          onStatus={(next) => {
            keep(next)
            setError(null)
          }}
        />
      )}
      {dialog && (
        <AiSetupDialog
          onStatus={keep}
          onClose={() => setDialog(false)}
          onChooseOther={() => setDialog(false)}
        />
      )}
    </section>
  )
}
