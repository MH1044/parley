import { useState } from 'react'
import { t } from '../../strings'
import About from './About'
import AiSection from './AiSection'
import AudioSection from './AudioSection'
import Data from './Data'
import Learning from './Learning'
import SpeakingSection from './SpeakingSection'

export type SettingsTab = 'learning' | 'audio' | 'speaking' | 'ai' | 'data' | 'about'

interface Props {
  onBack: () => void
  /** The data was replaced or deleted: start the app again. */
  onRestart: () => void
  /** The part that opens first (Learning unless another is asked for). */
  initialTab?: SettingsTab
}

/** Settings in six parts: Learning, Audio, Speaking, AI, Data and About. */
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
  // Back to my path at the top as well as the foot, so a long part never has to be scrolled.
  const backToPath = (
    <div className="actions">
      <button type="button" onClick={onBack}>
        {t('app.back_to_path')}
      </button>
    </div>
  )
  return (
    <main className="settings">
      <h1>{t('settings.title')}</h1>
      {backToPath}
      <nav className="tabs">
        {tabButton('learning', t('settings.learning'))}
        {tabButton('audio', t('settings.audio'))}
        {tabButton('speaking', t('speaking.tab'))}
        {tabButton('ai', t('ai.settings.tab'))}
        {tabButton('data', t('settings.data'))}
        {tabButton('about', t('settings.about'))}
      </nav>
      {tab === 'learning' && <Learning />}
      {tab === 'audio' && <AudioSection />}
      {tab === 'speaking' && <SpeakingSection />}
      {tab === 'ai' && <AiSection />}
      {tab === 'data' && <Data onRestart={onRestart} />}
      {tab === 'about' && <About />}
      {backToPath}
    </main>
  )
}
