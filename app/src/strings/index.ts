// Every string the interface shows comes from en.json through t(), so the copy can be read and
// checked in one place. Keys are flat; values may hold {name} placeholders.

import strings from './en.json'

export type StringKey = keyof typeof strings

const table: Record<string, string> = strings

/** The text of `key`, with each {name} replaced by vars[name]. */
export function t(key: StringKey, vars?: Record<string, string | number>): string {
  const template = table[key] ?? key
  if (!vars) return template
  return template.replace(/\{(\w+)\}/g, (whole: string, name: string) =>
    name in vars ? String(vars[name]) : whole,
  )
}

/** The English name of a course language code ("es" → "Spanish"). */
export function languageName(code: string): string {
  switch (code) {
    case 'es':
      return t('language.es')
    case 'fr':
      return t('language.fr')
    case 'de':
      return t('language.de')
    default:
      return code
  }
}
