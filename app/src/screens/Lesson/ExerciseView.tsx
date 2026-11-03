import { useId } from 'react'
import type { Exercise } from '../../api/commands'
import MatchPairs from '../../components/MatchPairs'
import WordBank from '../../components/WordBank'
import { t } from '../../strings'
import { usesWordBank, type Draft } from './draft'

interface Props {
  exercise: Exercise
  /** The course language's code, for `lang` attributes. */
  lang: string
  /** The course language's English name. */
  language: string
  wordBank: boolean
  draft: Draft
  checked: boolean
  onDraft: (draft: Draft) => void
}

interface AnswerBoxProps {
  value: string
  lang: string
  disabled: boolean
  onChange: (value: string) => void
}

/** The text box a typed answer goes in. */
function AnswerBox({ value, lang, disabled, onChange }: AnswerBoxProps) {
  const id = useId()
  return (
    <div className="answer">
      <label htmlFor={id}>{t('exercise.answer')}</label>
      <input
        id={id}
        type="text"
        lang={lang}
        value={value}
        disabled={disabled}
        autoComplete="off"
        autoCorrect="off"
        spellCheck={false}
        autoFocus
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  )
}

/** The instruction, the prompt and the answer input of one exercise. */
export default function ExerciseView({
  exercise,
  lang,
  language,
  wordBank,
  draft,
  checked,
  onDraft,
}: Props) {
  const textBox = (answerLang: string) => (
    <AnswerBox
      value={draft.text}
      lang={answerLang}
      disabled={checked}
      onChange={(text) => onDraft({ ...draft, text })}
    />
  )

  switch (exercise.type) {
    case 'pick_meaning':
      return (
        <div className="exercise">
          <h1>{t('exercise.pick_meaning', { target: exercise.prompt_target })}</h1>
          <div role="radiogroup" aria-label={t('exercise.options')} className="options">
            {exercise.options.map((option, index) => (
              <label key={`${exercise.id}-${index}`} className="option">
                <input
                  type="radio"
                  name={`options-${exercise.id}`}
                  value={index}
                  checked={draft.choice === index}
                  disabled={checked}
                  onChange={() => onDraft({ ...draft, choice: index })}
                />
                <span>{option}</span>
              </label>
            ))}
          </div>
        </div>
      )
    case 'translate_to_target':
      return (
        <div className="exercise">
          <h1>{t('exercise.translate_to_target', { language })}</h1>
          <p className="prompt" lang="en">
            {exercise.prompt_en}
          </p>
          {usesWordBank(exercise, wordBank) ? (
            <WordBank
              tiles={exercise.tiles ?? []}
              chosen={draft.tiles}
              disabled={checked}
              onChange={(chosen) => onDraft({ ...draft, tiles: chosen })}
            />
          ) : (
            textBox(lang)
          )}
        </div>
      )
    case 'translate_to_en':
      return (
        <div className="exercise">
          <h1>{t('exercise.translate_to_en')}</h1>
          <p className="prompt" lang={lang}>
            {exercise.prompt_target}
          </p>
          {textBox('en')}
        </div>
      )
    case 'fill_gap':
      return (
        <div className="exercise">
          <h1>{t('exercise.fill_gap')}</h1>
          <p className="prompt" lang={lang}>
            {exercise.sentence}
          </p>
          <p className="hint">
            <span className="label">{t('exercise.hint')}</span>{' '}
            <span lang="en">{exercise.hint_en}</span>
          </p>
          {textBox(lang)}
        </div>
      )
    case 'listen_type':
      return (
        <div className="exercise">
          <h1>{t('exercise.listen_type')}</h1>
          <p className="prompt" lang={lang}>
            {exercise.text}
          </p>
          {textBox(lang)}
        </div>
      )
    case 'speak':
      return (
        <div className="exercise">
          <h1>{t('exercise.speak')}</h1>
          <p className="prompt" lang={lang}>
            {exercise.text}
          </p>
          {textBox(lang)}
        </div>
      )
    case 'match_pairs':
      return (
        <div className="exercise">
          <h1>{t('exercise.match_pairs')}</h1>
          <MatchPairs
            pairs={exercise.pairs}
            matched={draft.pairs}
            disabled={checked}
            onChange={(pairs) => onDraft({ ...draft, pairs })}
          />
        </div>
      )
  }
}
