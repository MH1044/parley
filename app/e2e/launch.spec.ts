// Module 04 acceptance test 16, the launch smoke test: the debug build starts and shows the
// Welcome text within 20 s. It drives tauri-driver over the W3C WebDriver protocol with fetch, so
// no WebDriver client package is needed. Run with `npm --prefix app run e2e` after building the
// front end and the debug app; tauri-driver and the platform's WebDriver must be on the PATH.
//
// Environment: TAURI_DRIVER (default `tauri-driver`), TAURI_DRIVER_ARGS (extra arguments,
// separated by spaces) and PARLEY_E2E_APP (default target/debug/parley-app, `.exe` on Windows).

import { spawn, type ChildProcess } from 'node:child_process'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

const DRIVER = 'http://127.0.0.1:4444'
const WELCOME = 'Learn by speaking'
const DRIVER_READY_MS = 30_000
const WELCOME_WITHIN_MS = 20_000

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..')
const application =
  process.env.PARLEY_E2E_APP ??
  join(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'parley-app.exe' : 'parley-app')

let driver: ChildProcess | undefined
let driverFailure: Error | undefined
let sessionId: string | undefined

const sleep = (ms: number) => new Promise<void>((done) => setTimeout(done, ms))

interface Reply {
  status: number
  value: Record<string, unknown> | null
}

async function webdriver(method: string, path: string, body?: unknown): Promise<Reply> {
  const response = await fetch(`${DRIVER}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  return { status: response.status, value: valueOf(await response.text()) }
}

/** The `value` of a WebDriver reply, or null when the body is not JSON. */
function valueOf(text: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(text) as { value?: unknown }
    return (parsed.value ?? null) as Record<string, unknown> | null
  } catch {
    return null
  }
}

async function waitForDriver() {
  const deadline = Date.now() + DRIVER_READY_MS
  for (;;) {
    if (driverFailure) throw driverFailure
    try {
      const response = await fetch(`${DRIVER}/status`)
      if (response.ok) return
    } catch {
      // Not listening yet.
    }
    if (Date.now() > deadline) throw new Error(`tauri-driver did not answer at ${DRIVER}/status`)
    await sleep(250)
  }
}

beforeAll(async () => {
  const command = process.env.TAURI_DRIVER ?? 'tauri-driver'
  const args = (process.env.TAURI_DRIVER_ARGS ?? '').split(' ').filter((arg) => arg.length > 0)
  driver = spawn(command, args, { stdio: 'inherit' })
  driver.on('error', (error) => {
    driverFailure = error
  })
  await waitForDriver()
}, DRIVER_READY_MS + 5_000)

afterAll(async () => {
  if (sessionId !== undefined) {
    try {
      await webdriver('DELETE', `/session/${sessionId}`)
    } catch {
      // The driver may already be gone.
    }
  }
  driver?.kill()
})

describe('launch smoke (acceptance test 16)', () => {
  it(`shows "${WELCOME}" within ${WELCOME_WITHIN_MS / 1000} s of starting`, async () => {
    const created = await webdriver('POST', '/session', {
      capabilities: { alwaysMatch: { 'tauri:options': { application } } },
    })
    expect(created.status, JSON.stringify(created.value)).toBe(200)
    sessionId = created.value?.sessionId as string | undefined
    expect(sessionId, 'a WebDriver session id').toBeTypeOf('string')
    const started = Date.now()

    let found = false
    while (!found && Date.now() - started <= WELCOME_WITHIN_MS) {
      const element = await webdriver('POST', `/session/${sessionId}/element`, {
        using: 'xpath',
        value: `//*[contains(normalize-space(.), '${WELCOME}')]`,
      })
      found = element.status === 200
      if (!found) await sleep(250)
    }
    const elapsed = Date.now() - started
    expect(found, `"${WELCOME}" appeared`).toBe(true)
    expect(elapsed).toBeLessThanOrEqual(WELCOME_WITHIN_MS)
  })
})
