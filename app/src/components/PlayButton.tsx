import { useEffect, useRef, useState } from 'react'
import {
  asAppError,
  onAudioEnded,
  ttsPlay,
  ttsStop,
  type AppError,
} from '../api/commands'
import { t } from '../strings'
import AiBanner from './AiBanner'

interface Props {
  /** The sentence to read, in the course language. */
  text: string
  /** The course language's code. */
  lang: string
  /** The lesson a natural voice's charge is added to; null outside a lesson. */
  sessionId?: string | null
  /** Play once, at normal speed, as soon as this is true (the autoplay setting). */
  autoPlay?: boolean
}

/**
 * Play and Slow for one sentence. While it plays, Stop takes their place until `audio://ended`.
 * Under the buttons: the notice of a fallback to the system voice, a natural voice's AI error as
 * the AI banner shows it (a 402), and the message of a `no_voice` error (no voice installed, or
 * no speaker). Other failures leave the buttons as they were.
 */
export default function PlayButton({ text, lang, sessionId = null, autoPlay = false }: Props) {
  const [playing, setPlaying] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [aiError, setAiError] = useState<AppError | null>(null)
  const [failure, setFailure] = useState<string | null>(null)
  const autoPlayed = useRef(false)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    const unlisten = onAudioEnded(() => {
      if (mounted.current) setPlaying(false)
    })
    return () => {
      mounted.current = false
      void unlisten.then((stop) => stop())
    }
  }, [])

  async function play(slow: boolean) {
    setNotice(null)
    setAiError(null)
    setFailure(null)
    setPlaying(true)
    try {
      const played = await ttsPlay(text, lang, slow, sessionId)
      if (!mounted.current) return
      setNotice(played?.notice ?? null)
      setAiError(played?.ai_error ?? null)
    } catch (reason) {
      if (!mounted.current) return
      setPlaying(false)
      const error = asAppError(reason)
      if (error?.code === 'no_voice') setFailure(error.message)
    }
  }

  async function stop() {
    setPlaying(false)
    try {
      await ttsStop()
    } catch {
      // Nothing was playing.
    }
  }

  // Autoplay: once per sentence shown, the first time the setting allows it.
  useEffect(() => {
    if (autoPlay && !autoPlayed.current) {
      autoPlayed.current = true
      void play(false)
    }
    // play reads the props of this render; it runs once, so it need not be a dependency.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoPlay])

  return (
    <div className="play-button">
      <div className="actions">
        {playing ? (
          <button type="button" onClick={() => void stop()}>
            {t('audio.stop')}
          </button>
        ) : (
          <>
            <button type="button" aria-label={t('audio.play_label')} onClick={() => void play(false)}>
              {t('audio.play')}
            </button>
            <button type="button" aria-label={t('audio.slow_label')} onClick={() => void play(true)}>
              {t('audio.slow')}
            </button>
          </>
        )}
      </div>
      {notice && <p className="notice">{notice}</p>}
      {aiError && <AiBanner error={aiError} />}
      {failure && <p className="error">{failure}</p>}
    </div>
  )
}
