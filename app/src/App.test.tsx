import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

// The Tauri bridge does not exist under jsdom, so the commands are faked here. The version app_info
// answers with is deliberately not the real one: the test proves the page shows what the command
// says.
const answers = vi.hoisted(
  (): Record<string, unknown> => ({
    app_start: { onboarded: true, active_course: 'es', notice: null },
    app_info: { version: '9.9.9', data_dir: 'parley-data-folder' },
    settings_get: {
      schema: 'parley.settings/1',
      active_course: 'es',
      daily_goal_xp: 20,
      accent_leniency: 'lenient',
      answer_input: 'typing',
      desired_retention: 0.9,
      new_items_per_lesson: 5,
      lesson_length: 12,
      audio: {
        voice_source: 'os',
        os_voice: { es: null, fr: null, de: null },
        natural_voice: { es: null, fr: null, de: null },
        slow_speed: 0.75,
        autoplay: true,
      },
      speaking: { enabled: true, transcriber: 'provider', pass_threshold: 0.8, keep_recordings: false },
      ai: {
        provider: 'unset',
        fresh_lessons: true,
        prepare_next_lesson: true,
        stories: true,
        share_device_name: false,
        own_key: { base_url: null, chat_model: null, transcribe_model: null, speech_model: null, speech_voice: null },
        local: { base_url: 'http://localhost:11434/v1', chat_model: null },
      },
      update_check: true,
    },
    course_list: [
      { lang: 'es', name: 'Spanish', started: true, active: true, placement_done: false, cefr_estimate: null, skills: 8 },
    ],
    pack_status: { lang: 'es', state: 'not_published', version: null, bytes: null, size: null },
    path_get: {
      lang: 'es',
      units: [
        {
          id: 'es.a1.u01',
          n: 1,
          title: 'Hello',
          cefr: 'A1',
          skills: [
            {
              id: 'es.a1.u01.s01',
              title: 'Greetings',
              can_do: 'I can say hello and goodbye and ask how someone is.',
              cefr: 'A1',
              state: 'available',
              mastery: 0,
              tested_out: false,
            },
          ],
        },
      ],
      next_skill_id: 'es.a1.u01.s01',
      due: 0,
      streak: 0,
      freezes: 0,
      today_xp: 0,
      goal_xp: 20,
    },
  }),
)

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => {
    if (command in answers) return answers[command]
    return Promise.reject({ code: 'not_found', message: `No answer for ${command}.`, action: null })
  }),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}))

import App from './App'

afterEach(() => {
  cleanup()
})

describe('App', () => {
  // Module 00 acceptance test 10: the app shows the version answered by app_info. Updated in
  // module 04 because the first screen is now onboarding: an onboarded learner finds the version
  // on Settings, About.
  it('shows the version answered by app_info on the About page', async () => {
    render(<App />)
    fireEvent.click(await screen.findByRole('button', { name: 'Settings' }))
    fireEvent.click(await screen.findByRole('button', { name: 'About' }))
    const version = await screen.findAllByText(
      (_content, element) =>
        element !== null &&
        (element.textContent ?? '').includes('9.9.9') &&
        !Array.from(element.children).some((child) => (child.textContent ?? '').includes('9.9.9')),
    )
    expect(version.length).toBeGreaterThan(0)
  })
})
