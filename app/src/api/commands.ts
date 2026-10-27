// Typed wrappers for every Tauri command and event.
//
// Argument and result types are defined in crates/parley-core/src/api.rs and mirrored here by hand;
// a test compares the two lists of command names, so keep COMMANDS in the same order as the Rust
// list. The UI never holds a key and never builds a URL of its own: it only sends intents and
// renders results.

import { invoke } from '@tauri-apps/api/core'

/** Every command the core exposes, mirroring `parley_core::api::COMMANDS`. */
export const COMMANDS = ['app_info'] as const

export interface AppInfo {
  version: string
  data_dir: string
}

/** The running version and the app data directory. */
export function appInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('app_info')
}
