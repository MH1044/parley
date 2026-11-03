import { useState } from 'react'
import { backupImport, errorMessage, filePick } from '../../api/commands'
import { t } from '../../strings'

interface Props {
  /** What the store said, shown on request. */
  details: string
  /** A backup was restored: start the app again. */
  onRestart: () => void
}

/**
 * The data file cannot be opened. The learner can restore a backup (nothing here can be lost:
 * the file cannot be read), or see the details to report the problem.
 */
export default function DataError({ details, onRestart }: Props) {
  const [showDetails, setShowDetails] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  async function restore() {
    setBusy(true)
    setError(null)
    try {
      const path = await filePick('backup_import')
      if (!path) return
      await backupImport(path)
      onRestart()
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="data-error">
      <h1>{t('data_error.title')}</h1>
      <div className="actions">
        <button type="button" className="primary" disabled={busy} onClick={() => void restore()}>
          {t('data_error.restore')}
        </button>
        <button type="button" aria-expanded={showDetails} onClick={() => setShowDetails(!showDetails)}>
          {t('data_error.show_details')}
        </button>
      </div>
      {showDetails && <pre className="details">{details}</pre>}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </main>
  )
}
