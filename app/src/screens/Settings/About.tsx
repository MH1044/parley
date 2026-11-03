import { useEffect, useState } from 'react'
import { appInfo, type AppInfo } from '../../api/commands'
import { t } from '../../strings'

/** The version, where the data lives, and the credits for the open data the course uses. */
export default function About() {
  const [info, setInfo] = useState<AppInfo | null>(null)

  useEffect(() => {
    let cancelled = false
    appInfo().then(
      (found) => {
        if (!cancelled && found) setInfo(found)
      },
      () => {},
    )
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <section className="about" aria-label={t('settings.about')}>
      {info && (
        <dl>
          <dt>{t('settings.version')}</dt>
          <dd>{info.version}</dd>
          <dt>{t('settings.data_dir')}</dt>
          <dd>
            <code>{info.data_dir}</code>
          </dd>
        </dl>
      )}
      <p>{t('settings.tatoeba')}</p>
      <p>{t('settings.wiktionary')}</p>
      <p>{t('settings.licence')}</p>
    </section>
  )
}
