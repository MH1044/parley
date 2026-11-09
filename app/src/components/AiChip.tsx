import type { AiStatus } from '../api/commands'
import { t } from '../strings'
import BalanceLine from './BalanceLine'

/** Which provider answers AI requests, on the path; with publik API, the balance line under it. */
export default function AiChip({ status }: { status: AiStatus }) {
  let label: string
  switch (status.provider) {
    case 'publik':
      label = t('ai.chip.publik')
      break
    case 'own_key':
      label = t('ai.chip.own_key')
      break
    case 'local':
      label = t('ai.chip.local')
      break
    default:
      return null
  }
  return (
    <div className="ai-chip">
      <span className="chip">{label}</span>
      {status.provider === 'publik' && <BalanceLine line={status.balance_line} />}
    </div>
  )
}
