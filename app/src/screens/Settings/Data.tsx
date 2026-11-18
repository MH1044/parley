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

/** Where a message shows: under the buttons whose action it answers. */
type Place = 'data' | 'packs' | 'delete'

/** Backup export and restore, course packs, clearing caches, and deleting everything. */
export default function Data({ onRestart }: Props) {
  const [status, setStatus] = useState<{ text: string; at: Place } | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [confirmText, setConfirmText] = useState('')
  const [busy, setBusy] = useState(false)
  const deleteId = useId()
  const dialogTitle = useId()
  const packsTitle = useId()

  /** Runs an action; its message (what it returns, or the error's) shows at `at`. */
  async function run(at: Place, action: () => Promise<string | null | undefined | void>) {
    setBusy(true)
    setStatus(null)
    try {
      const text = await action()
      if (text) setStatus({ text, at })
    } catch (reason) {
      setStatus({ text: errorMessage(reason), at })
    } finally {
      setBusy(false)
    }
  }

  const statusAt = (at: Place) =>
    status?.at === at && (
      <p role="status" className="status">
        {status.text}
      </p>
    )

  const exportBackup = () =>
    run('data', async () => {
      const path = await filePick('backup_export')
      if (!path) return
      const summary = await backupExport(path)
      if (summary) return t('settings.exported', { cards: summary.cards, reviews: summary.reviews })
    })

  const restoreBackup = () => {
    setConfirming(false)
    return run('data', async () => {
      const path = await filePick('backup_import')
      if (!path) return
      await backupImport(path)
      onRestart()
    })
  }

  const exportCoursePack = () =>
    run('packs', async () => {
      const path = await filePick('course_pack_export')
      if (!path) return
      const settings = await settingsGet()
      const lang = settings?.active_course
      if (!lang) return
      const summary = await coursePackExport(lang, path)
      if (summary) return t('settings.course_pack_exported', { n: summary.lessons, m: summary.stories })
    })

  const importCoursePack = () =>
    run('packs', async () => {
      const path = await filePick('course_pack_import')
      if (!path) return
      const report = await coursePackImport(path)
      return report?.message
    })

  const clearCaches = () =>
    run('data', async () => {
      await cacheClear('all')
      return t('settings.caches_cleared')
    })

  // A failure shows under the button and the screen stays; when the data file is in use, nothing
  // was deleted.
  const deleteEverything = () =>
    run('delete', async () => {
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
      {statusAt('data')}
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
        {statusAt('packs')}
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
        {statusAt('delete')}
      </div>
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
