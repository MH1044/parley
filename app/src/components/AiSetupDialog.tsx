import { useId, useState } from 'react'
import {
  aiConsentAccept,
  aiConsentDecline,
  aiOpenLink,
  errorMessage,
  type AiStatus,
} from '../api/commands'
import { useEscape } from '../navigation'
import { t } from '../strings'
import BalanceLine from './BalanceLine'

interface Props {
  /**
   * The dialog closed: `true` after Continue on the first-run card (run the action that opened
   * it), `false` after Not now (the action is cancelled; nothing was sent).
   */
  onClose: (proceed: boolean) => void
  /** Use my own key or Use a local model: the learner wants Settings > AI instead. */
  onChooseOther: (which: 'own_key' | 'local') => void
  /** publik API was set up: the new status. */
  onStatus?: (status: AiStatus) => void
}

/**
 * The consent dialog (docs/PUBLIK-INTEGRATION.md §4): opened by an AI action the learner chose,
 * never at launch or in onboarding (invariant I12). It says what leaves the computer and shows
 * publik's disclosure verbatim; the three ways to get AI have equal weight. After Continue with
 * publik API, the first-run card shows the balance line, the cost sentence and the one link.
 */
export default function AiSetupDialog({ onClose, onChooseOther, onStatus }: Props) {
  const titleId = useId()
  const [status, setStatus] = useState<AiStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)

  async function accept() {
    setBusy(true)
    setFailure(null)
    try {
      const accepted = await aiConsentAccept()
      if (accepted) {
        setStatus(accepted)
        onStatus?.(accepted)
      }
    } catch (reason) {
      setFailure(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  async function notNow() {
    try {
      await aiConsentDecline()
    } catch {
      // Declining sends nothing and changes nothing; the dialog closes either way.
    }
    onClose(false)
  }

  async function openLink(kind: 'claim' | 'add_credit') {
    setFailure(null)
    try {
      await aiOpenLink(kind)
    } catch (reason) {
      setFailure(errorMessage(reason))
    }
  }

  // Esc closes the dialog without running the action that opened it: on the consent card it is
  // Not now; on the first-run card nothing more is sent. Not while publik API is being set up.
  useEscape(busy ? null : status ? () => onClose(false) : () => void notNow())

  const others = (
    <>
      <button type="button" disabled={busy} onClick={() => onChooseOther('own_key')}>
        {t('ai.setup.own_key')}
      </button>
      <button type="button" disabled={busy} onClick={() => onChooseOther('local')}>
        {t('ai.setup.local')}
      </button>
    </>
  )

  return (
    <div className="dialog-backdrop">
      <div role="dialog" aria-modal="true" aria-labelledby={titleId} className="dialog ai-setup">
        <h2 id={titleId}>{t('ai.setup.title')}</h2>
        {status ? (
          <>
            <BalanceLine line={status.balance_line} />
            {status.cost_sentence && <p>{status.cost_sentence}</p>}
            <div className="actions">
              {status.claim_state === 'claimed' ? (
                <button
                  type="button"
                  className="primary"
                  onClick={() => void openLink('add_credit')}
                >
                  {t('ai.link.add_credit')}
                </button>
              ) : (
                <button type="button" className="primary" onClick={() => void openLink('claim')}>
                  {t('ai.link.sign_in')}
                </button>
              )}
              {others}
              <button type="button" onClick={() => onClose(true)}>
                {t('ai.setup.continue')}
              </button>
            </div>
          </>
        ) : (
          <>
            <p>{t('ai.setup.data_path')}</p>
            <p>{t('ai.setup.disclosure')}</p>
            <p>{t('ai.setup.works_without')}</p>
            <div className="actions equal">
              <button type="button" disabled={busy} onClick={() => void accept()}>
                {t('ai.setup.continue_publik')}
              </button>
              {others}
            </div>
            {busy && <p role="status">{t('ai.setup.working')}</p>}
            <button
              type="button"
              className="link"
              disabled={busy}
              onClick={() => void notNow()}
            >
              {t('ai.setup.not_now')}
            </button>
          </>
        )}
        {failure && (
          <p role="alert" className="error">
            {failure}
          </p>
        )}
      </div>
    </div>
  )
}
