import { rowLetter, type HalfPair } from '../screens/Lesson/draft'
import { t } from '../strings'

interface Props {
  /** [target, English] pairs as the exercise gives them. */
  pairs: [string, string][]
  /** The meanings column, top to bottom, as pair indexes (`meaningOrder`). */
  rows: number[]
  /** Pairs made: [word index, index of the pair whose English was chosen]. */
  matched: [number, number][]
  /** The side chosen and waiting for its other half. */
  half: HalfPair | null
  disabled?: boolean
  onChoose: (choice: HalfPair) => void
  onStartOver: () => void
}

/**
 * Two columns of buttons, the words numbered 1 to n in the exercise's order and the English
 * meanings lettered A on in the order of `rows`; a word and a meaning, chosen either way round,
 * make a pair. A made pair takes its word's number: both halves show it in the pair's colour, the
 * meaning's letter giving way to it. Nothing is added or removed as pairs are made, so nothing
 * moves: **Start over** is always under the columns, disabled until there is a pair.
 */
export default function MatchPairs({
  pairs,
  rows,
  matched,
  half,
  disabled,
  onChoose,
  onStartOver,
}: Props) {
  const wordOfMeaning = (meaning: number) => matched.find(([, right]) => right === meaning)?.[0]
  const isMatched = (word: number) => matched.some(([left]) => left === word)

  /** A button of either column; `pairNumber` is set once it is in a pair. */
  const side = (
    choice: HalfPair,
    text: string,
    key: string,
    pairNumber: number | undefined,
  ) => {
    const made = pairNumber !== undefined
    const pressed = half !== null && half.side === choice.side && half.index === choice.index
    return (
      <button
        key={`${choice.side}-${choice.index}`}
        type="button"
        className={made ? `match matched pair-${pairNumber}` : 'match'}
        aria-pressed={pressed}
        aria-keyshortcuts={made ? undefined : key}
        aria-label={made ? t('match.paired', { text, n: pairNumber }) : undefined}
        disabled={disabled || made}
        onClick={() => onChoose(choice)}
      >
        <span className="match-key" aria-hidden="true">
          {made ? pairNumber : key}
        </span>
        <span className="match-text">{text}</span>
      </button>
    )
  }

  return (
    <div className="match-pairs">
      <div className="match-columns">
        <div className="match-column" role="group" aria-label={t('match.targets')}>
          {pairs.map(([target], index) =>
            side(
              { side: 'word', index },
              target,
              String(index + 1),
              isMatched(index) ? index + 1 : undefined,
            ),
          )}
        </div>
        <div className="match-column" role="group" aria-label={t('match.meanings')}>
          {rows.map((index, row) => {
            const word = wordOfMeaning(index)
            return side(
              { side: 'meaning', index },
              pairs[index]?.[1] ?? '',
              rowLetter(row),
              word === undefined ? undefined : word + 1,
            )
          })}
        </div>
      </div>
      <button
        type="button"
        className="link match-start-over"
        disabled={disabled || matched.length === 0}
        onClick={onStartOver}
      >
        {t('match.start_over')}
      </button>
    </div>
  )
}
