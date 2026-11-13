import { useId, useState } from 'react'
import {
  backupExport,
  backupImport,
  cacheClear,
  coursePackExport,
  coursePackImport,
  dataDeleteAll,
  errorMessage,
  filePick,
  settingsGet,
} from '../../api/commands'
import { useEscape } from '../../navigation'
import { t } from '../../strings'

interface Props {
  /** The data was replaced or deleted: start the app again. */
  onRestart: () => void
}

/** Backup export and restore, course packs, clearing caches, and deleting everything. */
export default function Data({ onRestart }: Props) {
  const [status, setStatus] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [confirmText, setConfirmText] = useState('')
  const [busy, setBusy] = useState(false)
  const deleteId = useId()
  const dialogTitle = useId()
  const packsTitle = useId()

  async function run(action: () => Promise<void>) {
    setBusy(true)
    setStatus(null)
    try {
      await action()
    } catch (reason) {
      setStatus(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  const exportBackup = () =>
    run(async () => {
      const path = await filePick('backup_export')
      if (!path) return
      const summary = await backupExport(path)
      if (summary) {
        setStatus(t('settings.exported', { cards: summary.cards, reviews: summary.reviews }))
      }
    })

  const restoreBackup = () => {
    setConfirming(false)
    return run(async () => {
      const path = await filePick('backup_import')
      if (!path) return
      await backupImport(path)
      onRestart()
    })
  }

  const exportCoursePack = () =>
    run(async () => {
      const path = await filePick('course_pack_export')
      if (!path) return
      const settings = await settingsGet()
      const lang = settings?.active_course
      if (!lang) return
      const summary = await coursePackExport(lang, path)
      if (summary) {
        setStatus(t('settings.course_pack_exported', { n: summary.lessons, m: summary.stories }))
      }
    })

  const importCoursePack = () =>
    run(async () => {
      const path = await filePick('course_pack_import')
      if (!path) return
      const report = await coursePackImport(path)
      if (report) setStatus(report.message)
    })

  const clearCaches = () =>
    run(async () => {
      await cacheClear('all')
      setStatus(t('settings.caches_cleared'))
    })

  const deleteEverything = () =>
    run(async () => {
      await dataDeleteAll(confirmText)
      onRestart()
    })

  // Esc in the restore dialog is Cancel.
  useEscape(confirming ? () => setConfirming(false) : null)

  return (
    <section className="data" aria-label={t('settings.data')}>
      <div className="actions">
        <button type="button" disabled={busy} onClick={() => void exportBackup()}>
          {t('settings.export')}
        </button>
        <button type="button" disabled={busy} onClick={() => setConfirming(true)}>
          {t('settings.restore')}
        </button>
        <button type="button" disabled={busy} onClick={() => void clearCaches()}>
          {t('settings.clear_caches')}
        </button>
      </div>
      <section className="course-packs" aria-labelledby={packsTitle}>
        <h2 id={packsTitle}>{t('settings.course_packs')}</h2>
        <div className="actions">
          <button type="button" disabled={busy} onClick={() => void exportCoursePack()}>
            {t('settings.course_pack_export')}
          </button>
          <button type="button" disabled={busy} onClick={() => void importCoursePack()}>
            {t('settings.course_pack_import')}
          </button>
        </div>
      </section>
      <div className="danger-zone">
        <label htmlFor={deleteId}>{t('settings.delete_label')}</label>
        <input
          id={deleteId}
          type="text"
          autoComplete="off"
          value={confirmText}
          onChange={(event) => setConfirmText(event.target.value)}
        />
        <button
          type="button"
          className="danger"
          disabled={busy}
          onClick={() => void deleteEverything()}
        >
          {t('settings.delete')}
        </button>
      </div>
      {status && (
        <p role="status" className="status">
          {status}
        </p>
      )}
      {confirming && (
        <div className="dialog-backdrop">
          <div role="dialog" aria-modal="true" aria-labelledby={dialogTitle} className="dialog">
            <p id={dialogTitle}>{t('settings.restore_warning')}</p>
            <div className="actions">
              <button type="button" className="danger" onClick={() => void restoreBackup()}>
                {t('settings.replace')}
              </button>
              <button type="button" autoFocus onClick={() => setConfirming(false)}>
                {t('settings.cancel')}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  )
}
