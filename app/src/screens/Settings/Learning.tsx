import { useEffect, useId, useState } from 'react'
import { errorMessage, settingsGet, settingsSet, type Settings } from '../../api/commands'
import { t } from '../../strings'

const GOALS = [10, 20, 30, 50]
const LENGTHS = [8, 10, 12, 15, 20]
const NEW_ITEMS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
const RETENTIONS = [0.8, 0.85, 0.9, 0.95]

interface ChoiceProps {
  label: string
  value: string
  options: { value: string; label: string }[]
  onChange: (value: string) => void
}

function Choice({ label, value, options, onChange }: ChoiceProps) {
  const id = useId()
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <select id={id} value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  )
}

const numbers = (values: number[], label: (n: number) => string) =>
  values.map((n) => ({ value: String(n), label: label(n) }))

/** The daily goal, accent leniency, answer input, lesson length, new words and target recall. */
export default function Learning() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [status, setStatus] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    settingsGet().then(
      (found) => {
        if (!cancelled && found) setSettings(found)
      },
      (reason: unknown) => {
        if (!cancelled) setStatus(errorMessage(reason))
      },
    )
    return () => {
      cancelled = true
    }
  }, [])

  async function change(patch: Record<string, unknown>) {
    try {
      const saved = await settingsSet(patch)
      if (saved) setSettings(saved)
      setStatus(t('settings.saved'))
    } catch (reason) {
      setStatus(errorMessage(reason))
    }
  }

  if (!settings) {
    return status ? <p role="alert">{status}</p> : null
  }

  return (
    <section className="learning" aria-label={t('settings.learning')}>
      <Choice
        label={t('settings.daily_goal')}
        value={String(settings.daily_goal_xp)}
        options={numbers(GOALS, (xp) => t('goal.option', { xp }))}
        onChange={(value) => void change({ daily_goal_xp: Number(value) })}
      />
      <Choice
        label={t('settings.accent')}
        value={settings.accent_leniency}
        options={[
          { value: 'lenient', label: t('settings.accent_lenient') },
          { value: 'strict', label: t('settings.accent_strict') },
          { value: 'ignore', label: t('settings.accent_ignore') },
        ]}
        onChange={(value) => void change({ accent_leniency: value })}
      />
      <Choice
        label={t('settings.answer_input')}
        value={settings.answer_input}
        options={[
          { value: 'typing', label: t('settings.answer_typing') },
          { value: 'word_bank', label: t('settings.answer_word_bank') },
        ]}
        onChange={(value) => void change({ answer_input: value })}
      />
      <Choice
        label={t('settings.lesson_length')}
        value={String(settings.lesson_length)}
        options={numbers(
          LENGTHS.includes(settings.lesson_length)
            ? LENGTHS
            : [...LENGTHS, settings.lesson_length].sort((a, b) => a - b),
          String,
        )}
        onChange={(value) => void change({ lesson_length: Number(value) })}
      />
      <Choice
        label={t('settings.new_items')}
        value={String(settings.new_items_per_lesson)}
        options={numbers(NEW_ITEMS, String)}
        onChange={(value) => void change({ new_items_per_lesson: Number(value) })}
      />
      <Choice
        label={t('settings.retention')}
        value={String(
          RETENTIONS.reduce((best, r) =>
            Math.abs(r - settings.desired_retention) < Math.abs(best - settings.desired_retention)
              ? r
              : best,
          ),
        )}
        options={numbers(RETENTIONS, (r) =>
          t('settings.retention_option', { percent: Math.round(r * 100) }),
        )}
        onChange={(value) => void change({ desired_retention: Number(value) })}
      />
      {status && (
        <p role="status" className="status">
          {status}
        </p>
      )}
    </section>
  )
}
