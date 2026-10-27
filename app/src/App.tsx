import { useEffect, useState } from 'react'
import { appInfo, type AppInfo } from './api/commands'

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    appInfo().then(
      (result) => {
        if (!cancelled) setInfo(result)
      },
      (reason: unknown) => {
        if (!cancelled) setError(String(reason))
      },
    )
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <main>
      <h1>Parley</h1>
      <p className="version">{info ? `Version ${info.version}` : (error ?? 'Loading…')}</p>
    </main>
  )
}
