#!/usr/bin/env node
// Licence gate for the front end (CONTRIBUTING.md, ADR-0002): reads a package-lock.json
// (lockfile version 2 or 3) and fails on any package, transitive ones included, whose licence is
// not permissive. Runs in scripts/verify and in CI next to `cargo deny check licenses`.
//
//   node scripts/check-npm-licences.mjs app/package-lock.json
//
// Exit 0 when every package is allowed, 1 with one line per offending package, 2 on usage errors.

import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'

// Permissive licences only. CC-BY-4.0 and CC0-1.0 cover data packages (browser compatibility
// tables); BlueOak-1.0.0 and Python-2.0 are permissive licences a few widely used packages carry.
const ALLOW = new Set(
  [
    'MIT',
    'MIT-0',
    'ISC',
    'BSD-2-Clause',
    'BSD-3-Clause',
    '0BSD',
    'Apache-2.0',
    'Unlicense',
    'CC0-1.0',
    'CC-BY-4.0',
    'MPL-2.0',
    'Zlib',
    'BlueOak-1.0.0',
    'Python-2.0',
    'Unicode-3.0',
    'Unicode-DFS-2016',
    'BSL-1.0',
  ].map((id) => id.toUpperCase()),
)

const MESSAGE = 'the licence gate allows only permissive licences'

function allowed(expression) {
  // SPDX expressions: `A`, `(A OR B)`, `A AND B`, `A WITH exception`; OR needs one allowed side,
  // AND needs both. Anything unparsable is not allowed.
  const tokens = expression.match(/\(|\)|[^\s()]+/g) ?? []
  let i = 0
  const peek = () => tokens[i]
  const next = () => tokens[i++]
  function term() {
    const t = next()
    if (t === undefined) return false
    if (t === '(') {
      const value = orExpr()
      if (next() !== ')') return false
      return value
    }
    let ok = ALLOW.has(t.toUpperCase())
    if (peek()?.toUpperCase() === 'WITH') {
      next()
      next() // the exception name does not change permissiveness for the gate's purposes
    }
    return ok
  }
  function andExpr() {
    let value = term()
    while (peek()?.toUpperCase() === 'AND') {
      next()
      value = term() && value
    }
    return value
  }
  function orExpr() {
    let value = andExpr()
    while (peek()?.toUpperCase() === 'OR') {
      next()
      value = andExpr() || value
    }
    return value
  }
  const value = orExpr()
  return value && i === tokens.length
}

function licenceOf(lockDir, path, entry) {
  let licence = entry.license
  if (licence === undefined) {
    // npm omits the field for a few packages; the installed package.json still has it.
    const packageJson = join(lockDir, path, 'package.json')
    if (existsSync(packageJson)) {
      const pkg = JSON.parse(readFileSync(packageJson, 'utf8'))
      licence = pkg.license ?? (Array.isArray(pkg.licenses) ? pkg.licenses.map((l) => l.type ?? l).join(' OR ') : undefined)
    }
  }
  if (licence && typeof licence === 'object') licence = licence.type
  return typeof licence === 'string' ? licence.trim() : undefined
}

const lockPath = process.argv[2]
if (!lockPath) {
  console.error('usage: node scripts/check-npm-licences.mjs <package-lock.json>')
  process.exit(2)
}

const lock = JSON.parse(readFileSync(lockPath, 'utf8'))
const packages = lock.packages
if (!packages || typeof packages !== 'object') {
  console.error(`${lockPath} has no "packages" object; lockfile version 2 or 3 is needed`)
  process.exit(2)
}

const failures = []
let checked = 0
for (const [path, entry] of Object.entries(packages)) {
  if (path === '' || entry.link) continue // the project itself, or a workspace link
  const name = entry.name ?? path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length)
  const licence = licenceOf(dirname(lockPath), path, entry)
  checked += 1
  if (licence === undefined) {
    failures.push(`${name}@${entry.version} has no licence field; ${MESSAGE}`)
  } else if (!allowed(licence)) {
    failures.push(`${name}@${entry.version} is ${licence}; ${MESSAGE}`)
  }
}

if (failures.length > 0) {
  for (const line of failures) console.error(line)
  process.exit(1)
}
console.log(`npm licence gate: ${checked} packages, all permissive`)
