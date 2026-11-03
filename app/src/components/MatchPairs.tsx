import { useState } from 'react'
import { t } from '../strings'

interface Props {
  /** [target, English] pairs as the exercise gives them. */
  pairs: [string, string][]
  /** Matches made: [target index, index of the pair whose English was chosen]. */
  matched: [number, number][]
  disabled?: boolean
  onChange: (matched: [number, number][]) => void
}

/**
 * Two columns of buttons: the targets in the exercise's order and the English meanings in
 * alphabetical order. Choosing a target and then a meaning makes a pair.
 */
export default function MatchPairs({ pairs, matched, disabled, onChange }: Props) {
  const [selected, setSelected] = useState<number | null>(null)
  const meanings = pairs
    .map(([, english], index) => ({ english, index }))
    .sort((a, b) => a.english.localeCompare(b.english))
  const pairOfTarget = (target: number) => matched.findIndex(([left]) => left === target)
  const pairOfMeaning = (meaning: number) => matched.findIndex(([, right]) => right === meaning)

  return (
    <div className="match-pairs">
      <div className="match-column" role="group" aria-label={t('match.targets')}>
        {pairs.map(([target], index) => {
          const pair = pairOfTarget(index)
          return (
            <button
              key={`t-${index}`}
              type="button"
              className={pair >= 0 ? `match matched pair-${pair + 1}` : 'match'}
              aria-pressed={selected === index}
              disabled={disabled || pair >= 0}
              onClick={() => setSelected(selected === index ? null : index)}
            >
              {target}
            </button>
          )
        })}
      </div>
      <div className="match-column" role="group" aria-label={t('match.meanings')}>
        {meanings.map(({ english, index }) => {
          const pair = pairOfMeaning(index)
          return (
            <button
              key={`m-${index}`}
              type="button"
              className={pair >= 0 ? `match matched pair-${pair + 1}` : 'match'}
              disabled={disabled || pair >= 0}
              onClick={() => {
                if (selected === null) return
                onChange([...matched, [selected, index]])
                setSelected(null)
              }}
            >
              {english}
            </button>
          )
        })}
      </div>
      {matched.length > 0 && !disabled && (
        <button
          type="button"
          className="link"
          onClick={() => {
            setSelected(null)
            onChange([])
          }}
        >
          {t('match.start_over')}
        </button>
      )}
    </div>
  )
}
