import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

// The Tauri bridge does not exist under jsdom, so app_info is faked here. The version it answers
// with is deliberately not the real one: the test proves the screen shows what the command says.
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === 'app_info') return { version: '9.9.9', data_dir: 'data' }
    throw new Error(`unknown command ${command}`)
  }),
}))

import App from './App'

describe('App', () => {
  // Module 00 acceptance test 10: App renders "Parley" and the version from a mocked app_info.
  it('renders the name and the version answered by app_info', async () => {
    render(<App />)
    expect(screen.getByRole('heading', { name: 'Parley' })).toBeInTheDocument()
    expect(await screen.findByText('Version 9.9.9')).toBeInTheDocument()
  })
})
