import { t } from '../strings'

/** The meter's range, in dBFS: the level of silence and of a full-scale signal. */
const FLOOR = -60
const TOP = 0

interface Props {
  /** The last level of the microphone, in dBFS (−60 to 0). */
  dbfs: number
}

/**
 * The microphone's level while a take is recorded, from the last `mic://level` event. A meter
 * (not a progress bar), so a screen reader reads it as a level; the bar's length is the level
 * above the floor.
 */
export default function LevelMeter({ dbfs }: Props) {
  const value = Math.round(Math.min(TOP, Math.max(FLOOR, dbfs)))
  const fill = ((value - FLOOR) / (TOP - FLOOR)) * 100
  return (
    <div
      role="meter"
      aria-label={t('speak.meter_label')}
      aria-valuemin={FLOOR}
      aria-valuemax={TOP}
      aria-valuenow={value}
      className="level-meter"
    >
      <div className="level-meter-fill" style={{ width: `${fill}%` }} />
    </div>
  )
}
