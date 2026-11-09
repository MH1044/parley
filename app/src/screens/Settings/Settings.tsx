import { useState } from 'react'
import { t } from '../../strings'
import About from './About'
import AiSection from './AiSection'
import Data from './Data'
import Learning from './Learning'

export type SettingsTab = 'learning' | 'ai' | 'data' | 'about'

interface Props {
  onBack: () => void
  /** The data was replaced or deleted: start the app again. */
  onRestart: () => void
  /** The part that opens first (Learning unless another is asked for). */
  initialTab?: SettingsTab
}

/** Settings in four parts: Learning, AI, Data and About. */
export default function Settings({ onBack, onRestart, initialTab }: Props) {
  const [tab, setTab] = useState<SettingsTab>(initialTab ?? 'learning')
  const tabButton = (name: SettingsTab, label: string) => (
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
        {tabButton('ai', t('ai.settings.tab'))}
        {tabButton('data', t('settings.data'))}
        {tabButton('about', t('settings.about'))}
      </nav>
      {tab === 'learning' && <Learning />}
      {tab === 'ai' && <AiSection />}
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
