import { useState } from 'react'
import {
  aiOpenLink,
  aiReconnectPublik,
  errorMessage,
  type AiStatus,
  type AppError,
} from '../api/commands'
import { t } from '../strings'

interface Props {
  error: AppError
  /** The status's claim state: the 402 link reads Sign in with publik until it is `claimed`. */
  claimState?: string | null
  /** Try again after `busy` and `ai_invalid`. */
  onRetry?: () => void
  /** Reconnect succeeded: the new status. */
  onStatus?: (status: AiStatus) => void
}

/**
 * The one thing an AI error says, and its one way out (docs/PUBLIK-INTEGRATION.md §9): a 402 shows
 * its message and exactly one link (the top-up link the core holds); a disconnected publik API
 * offers Reconnect; busy and unusable answers offer Try again; anything else shows its message.
 */
export default function AiBanner({ error, claimState, onRetry, onStatus }: Props) {
  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)

  async function act(action: () => Promise<unknown>) {
    setBusy(true)
    setFailure(null)
    try {
      await action()
    } catch (reason) {
      setFailure(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  const reconnect = () =>
    act(async () => {
      const status = await aiReconnectPublik()
      if (status) onStatus?.(status)
    })

  let body
  switch (error.code) {
    case 'needs_credit':
      body = (
        <>
          <p>{error.message}</p>
          <button
            type="button"
            className="link"
            disabled={busy}
            onClick={() => void act(() => aiOpenLink('top_up'))}
          >
            {claimState === 'claimed' ? t('ai.link.add_credit') : t('ai.link.sign_in')}
          </button>
          <p>{t('ai.banner.or_settings')}</p>
        </>
      )
      break
    case 'disconnected':
      body = (
        <>
          <p>{error.message}</p>
          <button type="button" disabled={busy} onClick={() => void reconnect()}>
            {t('ai.banner.reconnect')}
          </button>
        </>
      )
      break
    case 'busy':
    case 'ai_invalid':
      body = (
        <>
          <p>{error.message}</p>
          {onRetry && (
            <button type="button" disabled={busy} onClick={onRetry}>
              {t('ai.banner.try_again')}
            </button>
          )}
        </>
      )
      break
    default:
      body = <p>{error.message}</p>
  }

  return (
    <div role="alert" className="ai-banner">
      {body}
      {failure && <p className="error">{failure}</p>}
    </div>
  )
}
