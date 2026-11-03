import type { SkillState } from '../../api/commands'
import { t } from '../../strings'

/** The words a skill's state is shown with. */
export function stateLabel(state: SkillState): string {
  switch (state) {
    case 'locked':
      return t('state.locked')
    case 'available':
      return t('state.available')
    case 'learning':
      return t('state.learning')
    case 'practised':
      return t('state.practised')
    case 'mastered':
      return t('state.mastered')
    default:
      return String(state)
  }
}
