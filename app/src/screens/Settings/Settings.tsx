import { useState } from 'react'
import { t } from '../../strings'
import About from './About'
import Data from './Data'
import Learning from './Learning'

type Tab = 'learning' | 'data' | 'about'

interface Props {
  onBack: () => void
  /** The data was replaced or deleted: start the app again. */
  onRestart: () => void
}

/** Settings in three parts: Learning, Data and About. */
export default function Settings({ onBack, onRestart }: Props) {
  const [tab, setTab] = useState<Tab>('learning')
  const tabButton = (name: Tab, label: string) => (
    <button
      type="button"
      aria-pressed={tab === name}
      className={tab === name ? 'tab current' : 'tab'}
      onClick={() => setTab(name)}
    >
      {label}
    </button>
  )
  return (
    <main className="settings">
      <h1>{t('settings.title')}</h1>
      <nav className="tabs">
        {tabButton('learning', t('settings.learning'))}
        {tabButton('data', t('settings.data'))}
        {tabButton('about', t('settings.about'))}
      </nav>
      {tab === 'learning' && <Learning />}
      {tab === 'data' && <Data onRestart={onRestart} />}
      {tab === 'about' && <About />}
      <div className="actions">
        <button type="button" onClick={onBack}>
          {t('app.back_to_path')}
        </button>
      </div>
    </main>
  )
}
