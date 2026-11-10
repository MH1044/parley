import { useEffect, useState } from 'react'
import { t } from '../../strings'

/** How long a fresh lesson may take before the course pack is offered (module 07). */
export const COURSE_PACK_AFTER_MS = 5_000

interface Props {
  /** The learner chose the course pack: the lesson being written is no longer wanted. */
  onUseCoursePack: () => void
  busy?: boolean
}

/**
 * While a fresh lesson is being written: "Writing a fresh lesson…", and after 5 s the way out,
 * **Use the course pack instead**, which starts the offline lesson at once.
 */
export default function Loading({ onUseCoursePack, busy = false }: Props) {
  const [offer, setOffer] = useState(false)

  useEffect(() => {
    const timer = setTimeout(() => setOffer(true), COURSE_PACK_AFTER_MS)
    return () => clearTimeout(timer)
  }, [])

  return (
    <main className="loading-lesson">
      <p role="status">{t('lesson.writing')}</p>
      {offer && (
        <div className="actions">
          <button type="button" disabled={busy} onClick={onUseCoursePack}>
            {t('lesson.use_course_pack')}
          </button>
        </div>
      )}
    </main>
  )
}
