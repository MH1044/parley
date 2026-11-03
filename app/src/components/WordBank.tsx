import { t } from '../strings'

interface Props {
  tiles: string[]
  /** Indexes into `tiles`, in the order chosen. */
  chosen: number[]
  disabled?: boolean
  onChange: (chosen: number[]) => void
}

/** Builds an answer from tiles: a tile moves to the answer line, and back when pressed there. */
export default function WordBank({ tiles, chosen, disabled, onChange }: Props) {
  return (
    <div className="word-bank">
      <div className="word-bank-answer" role="group" aria-label={t('word_bank.chosen')}>
        {chosen.map((index, position) => (
          <button
            key={`${index}-${position}`}
            type="button"
            className="tile chosen"
            disabled={disabled}
            aria-label={t('word_bank.remove', { word: tiles[index] })}
            onClick={() => onChange(chosen.filter((_, p) => p !== position))}
          >
            {tiles[index]}
          </button>
        ))}
      </div>
      <div className="word-bank-tiles" role="group" aria-label={t('word_bank.label')}>
        {tiles.map((tile, index) => (
          <button
            key={`${tile}-${index}`}
            type="button"
            className="tile"
            disabled={disabled || chosen.includes(index)}
            onClick={() => onChange([...chosen, index])}
          >
            {tile}
          </button>
        ))}
      </div>
      {chosen.length > 0 && !disabled && (
        <button type="button" className="link" onClick={() => onChange([])}>
          {t('word_bank.clear')}
        </button>
      )}
    </div>
  )
}

