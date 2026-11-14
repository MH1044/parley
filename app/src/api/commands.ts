// Typed wrappers for every Tauri command and event.
//
// Argument and result types are defined in crates/parley-core/src/api.rs and mirrored here by hand;
// a test compares the two lists of command names, so keep COMMANDS in the same order as the Rust
// list. Arguments are passed in snake case, as the commands declare them. The UI never holds a key
// and never builds a URL of its own: it only sends intents and renders results.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/** Every command the core exposes, mirroring `parley_core::api::COMMANDS`. */
export const COMMANDS = [
  'app_start',
  'app_info',
  'file_pick',
  'data_delete_all',
  'cache_clear',
  'settings_get',
  'settings_set',
  'course_list',
  'course_start',
  'pack_status',
  'pack_download',
  'path_get',
  'skill_get',
  'placement_start',
  'placement_answer',
  'lesson_start',
  'review_start',
  'exercise_submit',
  'exercise_i_was_right',
  'lesson_finish',
  'stats_get',
  'backup_export',
  'backup_import',
  'ai_status',
  'ai_consent_accept',
  'ai_consent_decline',
  'ai_set_own_key',
  'ai_clear_own_key',
  'ai_local_probe',
  'ai_set_local',
  'ai_check_connection',
  'ai_disconnect_publik',
  'ai_reconnect_publik',
  'ai_open_link',
  // Module 07: lesson generation and course packs.
  'lesson_use_course_pack',
  'course_pack_export',
  'course_pack_import',
  'ai_lesson_checks',
  // Module 08: judging and explanations.
  'exercise_check',
  'explain_answer',
  // Module 09: listening and voices.
  'voices_list',
  'tts_play',
  'tts_stop',
  'tts_prefetch',
  'audio_status',
  'session_skip',
  // Module 10: speaking and pronunciation.
  'mic_start',
  'mic_stop',
  'recording_play',
  'speak_check',
  'whisper_models',
  'whisper_download',
  'speaking_status',
] as const

export type Command = (typeof COMMANDS)[number]

/** The event `pack_download` reports its progress on. */
export const PACK_PROGRESS_EVENT = 'pack://progress'

/** The event every AI command that changes the status emits it on. */
export const AI_STATUS_EVENT = 'ai://status'

/** The event `tts_play` emits when the clip or the OS voice it started ends. */
export const AUDIO_ENDED_EVENT = 'audio://ended'

/** The event `mic_start` emits with the level of every 50 ms of input. */
export const MIC_LEVEL_EVENT = 'mic://level'

/** The event `mic_start` emits once when the take reaches its limit. */
export const MIC_LIMIT_EVENT = 'mic://limit'

/** The event `whisper_download` reports its progress on. */
export const MODEL_PROGRESS_EVENT = 'model://progress'

// Errors -----------------------------------------------------------------------------------------

/** What a failed command rejects with. */
export interface AppError {
  code: string
  message: string
  action: Record<string, unknown> | null
}

/** The error a command rejected with, when it has the shape of an AppError. */
export function asAppError(reason: unknown): AppError | null {
  if (typeof reason === 'object' && reason !== null && 'message' in reason) {
    const r = reason as Partial<AppError>
    return {
      code: typeof r.code === 'string' ? r.code : '',
      message: String(r.message),
      action: (r.action as Record<string, unknown> | null | undefined) ?? null,
    }
  }
  return null
}

/** The message to show for a rejected command. */
export function errorMessage(reason: unknown): string {
  return asAppError(reason)?.message ?? String(reason)
}

// Result types -----------------------------------------------------------------------------------

export interface Launch {
  onboarded: boolean
  active_course: string | null
  notice: string | null
}

export interface AppInfo {
  version: string
  data_dir: string
}

export interface CacheCleared {
  files_removed: number
}

export interface PerLanguage {
  es: string | null
  fr: string | null
  de: string | null
}

/** settings.json (docs/ARCHITECTURE.md §4). Never holds a key. */
export interface Settings {
  schema: string
  active_course: string | null
  daily_goal_xp: number
  accent_leniency: 'lenient' | 'strict' | 'ignore'
  answer_input: 'typing' | 'word_bank'
  desired_retention: number
  new_items_per_lesson: number
  lesson_length: number
  audio: {
    voice_source: 'os' | 'natural'
    os_voice: PerLanguage
    natural_voice: PerLanguage
    slow_speed: number
    autoplay: boolean
  }
  speaking: {
    enabled: boolean
    transcriber: 'provider' | 'local'
    pass_threshold: number
    keep_recordings: boolean
    whisper_model: 'tiny' | 'base' | 'small'
  }
  ai: {
    provider: 'unset' | 'publik' | 'own_key' | 'local'
    fresh_lessons: boolean
    prepare_next_lesson: boolean
    stories: boolean
    share_device_name: boolean
    own_key: {
      base_url: string | null
      chat_model: string | null
      transcribe_model: string | null
      speech_model: string | null
      speech_voice: string | null
    }
    local: { base_url: string; chat_model: string | null }
  }
  update_check: boolean
}

export interface CourseSummary {
  lang: string
  name: string
  started: boolean
  active: boolean
  placement_done: boolean
  cefr_estimate: string | null
  skills: number
}

export type PackState = 'not_published' | 'available' | 'installed'

export interface PackStatus {
  lang: string
  state: PackState
  version: string | null
  bytes: number | null
  size: string | null
}

export interface PackProgress {
  lang: string
  bytes_done: number
  bytes_total: number
}

export type Cefr = 'A1' | 'A2'
export type SkillState = 'locked' | 'available' | 'learning' | 'practised' | 'mastered'

export interface PathSkill {
  id: string
  title: string
  can_do: string
  cefr: Cefr
  state: SkillState
  mastery: number
  tested_out: boolean
}

export interface PathUnit {
  id: string
  n: number
  title: string
  cefr: Cefr
  skills: PathSkill[]
  /** The unit's Story node while stories are on (module 07). */
  story?: PathStory | null
}

export interface PathStory {
  skill_id: string
  available: boolean
}

export interface PathView {
  lang: string
  units: PathUnit[]
  next_skill_id: string | null
  due: number
  streak: number
  freezes: number
  today_xp: number
  goal_xp: number
}

export interface Word {
  target: string
  en: string
}

export interface SkillDetail {
  id: string
  title: string
  can_do: string
  cefr: Cefr
  grammar_note: string | null
  unit_title: string
  state: SkillState
  mastery: number
  words: Word[]
  opened_by: string | null
}

export type Facet = 'receptive' | 'productive'
export type Origin = 'template' | 'ai' | 'course_pack'

interface ExerciseBase {
  id: string
  item_id: string
  facet: Facet
  origin: Origin
}

/** One exercise: the common fields and its type's own (schemas/lesson.schema.json). */
export type Exercise = ExerciseBase &
  (
    | { type: 'pick_meaning'; prompt_target: string; options: string[]; answer_index: number }
    | {
        type: 'translate_to_target'
        prompt_en: string
        target: string
        accepted: string[]
        tiles?: string[]
      }
    | { type: 'translate_to_en'; prompt_target: string; accepted: string[] }
    | { type: 'listen_type'; text: string; accepted: string[] }
    | { type: 'speak'; text: string; prompt_en: string }
    | { type: 'fill_gap'; sentence: string; answer: string; full_sentence: string; hint_en: string }
    | { type: 'match_pairs'; pairs: [string, string][] }
  )

/** A learner's answer: typed text, the index of a chosen option, or matched pair indexes. */
export type Answer = { text: string } | { choice: number } | { pairs: [number, number][] }

export type Verdict = 'correct' | 'almost' | 'typo' | 'wrong' | 'unchecked'

export interface Grade {
  verdict: Verdict
  tags: string[]
  message: string | null
  corrected: string | null
  words: string[]
  rating: number | null
  /** The AI judge's reason, on a verdict it gave (module 08). */
  explanation?: string | null
  /** The exercise's first accepted answer, under a judged wrong (module 08). */
  expected?: string | null
  /** Why an unchecked answer has no verdict: the AI judge's call failed (module 08). */
  judge_error?: AppError | null
}

/** The result of `explain_answer`. */
export interface Explained {
  text: string
  tip: string | null
  cached: boolean
}

export interface PlacementQuestion {
  session_id: string
  n: number
  skill_id: string
  exercise: Exercise
}

export interface PlacementOutcome {
  estimate: number
  label: string
  tested_out: number
  start_skill_id: string
  start_unit: number
  start_unit_title: string
}

export interface PlacementStep {
  next: PlacementQuestion | null
  result: PlacementOutcome | null
}

export interface Attribution {
  exercise_id: string
  author: string
}

export interface LessonStarted {
  session_id: string
  kind: 'lesson' | 'review' | 'story'
  skill_id: string | null
  origin: Origin
  exercises: Exercise[]
  attributions: Attribution[]
  notice: string | null
  /** Why a fresh lesson could not be written; the lesson came from the course pack (module 07). */
  ai_error?: AppError | null
  /** The story of a story session; its questions are answered as q1, q2, … (module 07). */
  story?: Story | null
}

export interface StoryQuestion {
  question_en: string
  options: string[]
  answer_index: number
  paragraph: number
}

export interface Story {
  title: string
  title_en: string
  paragraphs: string[]
  questions: StoryQuestion[]
}

export interface CoursePackSummary {
  bytes: number
  lessons: number
  stories: number
  exercises: number
}

export interface CoursePackReport {
  lessons: number
  stories: number
  dropped: number
  message: string
}

export interface LessonChecks {
  lessons: number
  valid: number
  planned: number
  message: string | null
}

export interface LessonSummary {
  xp: number
  correct: number
  total: number
  streak: number
  freezes: number
  record_week: boolean
  message: string | null
  daily_xp: number
  goal_xp: number
}

export interface StatsDay {
  local_date: string
  xp: number
  active: boolean
  frozen: boolean
}

export interface SkillProgress {
  id: string
  title: string
  state: SkillState
  mastery: number
}

export interface Stats {
  lang: string
  empty: boolean
  streak: number
  freezes: number
  days: StatsDay[]
  week_xp: number
  record_week_xp: number
  words_known: number
  vocab_items: number
  skills: SkillProgress[]
  cefr_estimate: string | null
  recall_rate: number | null
  reviews_30: number
}

export interface ExportSummary {
  bytes: number
  courses: number
  cards: number
  reviews: number
}

export interface ImportReport {
  courses: number
  items: number
  cards: number
  reviews: number
  skipped_cards: number
}

export type AiProvider = 'unset' | 'publik' | 'own_key' | 'local'

/** What the interface knows about AI. It never holds a key. */
export interface AiStatus {
  provider: AiProvider
  needs_consent: boolean
  connected: boolean
  claim_state: string | null
  balance_micros: number | null
  balance_line: string | null
  cost_sentence: string | null
  claim_url: string | null
  add_credit_url: string | null
  top_up_url: string | null
  needs_credit: boolean
  last_charge_micros: number | null
  last_request_id: string | null
  starter_remaining_micros: number | null
  week_used_micros: number | null
  week_budget_micros: number | null
  week_resets_at: string | null
  own_key_stored: boolean
  local_model: string | null
}

/** A model a local server offers, with its size in bytes when the server says. */
export interface LocalModel {
  name: string
  bytes: number | null
}

/** The result of the connection check. */
export interface ConnectionCheck {
  ok: boolean
  charge_micros: number | null
  message: string
}

/** The learner's own OpenAI-compatible endpoint. The key goes to the keychain and never comes back. */
export interface OwnKeyInput {
  base_url: string
  key: string
  chat_model: string
  transcribe_model: string | null
  speech_model: string | null
  speech_voice: string | null
}

/** The links of publik API the core holds; the interface names one, never a URL. */
export type AiLink = 'claim' | 'top_up' | 'add_credit'

/** One voice of `voices_list`: an installed OS voice, or the natural voice the settings name. */
export interface Voice {
  id: string
  name: string
  source: 'os' | 'natural'
  installed: boolean
}

export type VoiceSource = 'os' | 'natural'

/** The result of `tts_play`. */
export interface TtsPlayed {
  source: VoiceSource
  /** The natural sentence came from the audio cache. */
  cached: boolean
  /** "Natural voice unavailable, using the system voice." after a fallback. */
  notice: string | null
  /** The AI error to show as a banner (a 402). */
  ai_error: AppError | null
}

/** The result of `tts_prefetch`. */
export interface Prefetched {
  prefetched: boolean
}

/** The result of `audio_status`. */
export interface AudioStatus {
  /** "Natural voices cost about $0.00054 a sentence (measured)." */
  cost_line: string
  /** Until when listening exercises are off (ms since the epoch), while they are. */
  listen_off_until: number | null
}

/** The result of `mic_stop`: the take's id and its length after trimming. */
export interface Recording {
  id: string
  duration_ms: number
}

export type WordStatus = 'ok' | 'missed' | 'substituted'

/** One expected word of a speaking check, the heard word matched with it, and its status. */
export interface WordResult {
  expected: string
  heard: string | null
  status: WordStatus
}

/** The result of `speak_check` (module 10). */
export interface SpeakResult {
  words: WordResult[]
  extra: string[]
  score: number
  pass: boolean
  /** The transcript, with whisper.cpp's [...] spans removed. */
  heard: string
  /** The grade the result sheet shows: the first scored take's when this one rated nothing. */
  grade: Grade
  rated: boolean
  transcriber: 'provider' | 'local'
  cached: boolean
  notice: string | null
}

/** One local Whisper model of `whisper_models`. */
export interface ModelInfo {
  name: string
  bytes: number
  /** "148.0 MB". */
  size: string
  downloaded: boolean
  chosen: boolean
}

/** The payload of `model://progress`. */
export interface ModelProgress {
  name: string
  bytes_done: number
  bytes_total: number
}

/** The result of `speaking_status`. */
export interface SpeakingStatus {
  /** "Speaking checks cost about $0.00054 a take (measured)." */
  cost_line: string
  /** Until when speaking exercises are off (ms since the epoch), while they are. */
  speak_off_until: number | null
  /** This build has local Whisper. */
  local_available: boolean
  /** A microphone exists. */
  microphone: boolean
}

/** The payload of `mic://level`: the level of 50 ms of input, −60 to 0 dBFS. */
export interface MicLevel {
  dbfs: number
}

// Commands, in COMMANDS order --------------------------------------------------------------------

/**
 * Calls a command. A synchronous failure becomes a rejected promise, so every wrapper can be
 * awaited the same way.
 */
function call<T>(command: Command, args?: Record<string, unknown>): Promise<T> {
  try {
    return Promise.resolve(args === undefined ? invoke<T>(command) : invoke<T>(command, args))
  } catch (reason) {
    return Promise.reject(reason)
  }
}

/** The first command of a launch: whether onboarding is done, and any notice. */
export function appStart(): Promise<Launch> {
  return call<Launch>('app_start')
}

/** The running version and the app data directory. */
export function appInfo(): Promise<AppInfo> {
  return call<AppInfo>('app_info')
}

/** The native file dialog for a backup; the chosen path, or null when cancelled. */
export function filePick(
  kind: 'backup_export' | 'backup_import' | 'course_pack_export' | 'course_pack_import',
): Promise<string | null> {
  return call<string | null>('file_pick', { kind })
}

export function dataDeleteAll(confirmText: string): Promise<null> {
  return call<null>('data_delete_all', { confirm_text: confirmText })
}

export function cacheClear(kind: 'ai' | 'audio' | 'all'): Promise<CacheCleared> {
  return call<CacheCleared>('cache_clear', { kind })
}

export function settingsGet(): Promise<Settings> {
  return call<Settings>('settings_get')
}

/** Changes settings: an object of settings keys, nested objects merged key by key. */
export function settingsSet(patch: Record<string, unknown>): Promise<Settings> {
  return call<Settings>('settings_set', { patch })
}

export function courseList(): Promise<CourseSummary[]> {
  return call<CourseSummary[]>('course_list')
}

export function courseStart(lang: string): Promise<CourseSummary> {
  return call<CourseSummary>('course_start', { lang })
}

export function packStatus(lang: string): Promise<PackStatus> {
  return call<PackStatus>('pack_status', { lang })
}

/** Downloads the course pack; progress arrives on `pack://progress` (see onPackProgress). */
export function packDownload(lang: string): Promise<PackStatus> {
  return call<PackStatus>('pack_download', { lang })
}

export function pathGet(lang: string): Promise<PathView> {
  return call<PathView>('path_get', { lang })
}

export function skillGet(skillId: string): Promise<SkillDetail> {
  return call<SkillDetail>('skill_get', { skill_id: skillId })
}

export function placementStart(lang: string): Promise<PlacementQuestion> {
  return call<PlacementQuestion>('placement_start', { lang })
}

export function placementAnswer(sessionId: string, answer: Answer): Promise<PlacementStep> {
  return call<PlacementStep>('placement_answer', { session_id: sessionId, answer })
}

export function lessonStart(skillId: string, mode: 'learn' | 'review' | 'story'): Promise<LessonStarted> {
  return call<LessonStarted>('lesson_start', { skill_id: skillId, mode })
}

export function lessonUseCoursePack(skillId: string): Promise<LessonStarted> {
  return call<LessonStarted>('lesson_use_course_pack', { skill_id: skillId })
}

export function coursePackExport(lang: string, destPath: string): Promise<CoursePackSummary> {
  return call<CoursePackSummary>('course_pack_export', { lang, dest_path: destPath })
}

export function coursePackImport(srcPath: string): Promise<CoursePackReport> {
  return call<CoursePackReport>('course_pack_import', { src_path: srcPath })
}

export function aiLessonChecks(): Promise<LessonChecks> {
  return call<LessonChecks>('ai_lesson_checks')
}

export function reviewStart(lang: string, limit: number): Promise<LessonStarted> {
  return call<LessonStarted>('review_start', { lang, limit })
}

export function exerciseSubmit(
  sessionId: string,
  exerciseId: string,
  answer: Answer,
  responseMs: number | null,
  usedHint: boolean,
): Promise<Grade> {
  return call<Grade>('exercise_submit', {
    session_id: sessionId,
    exercise_id: exerciseId,
    answer,
    response_ms: responseMs,
    used_hint: usedHint,
  })
}

export function exerciseIWasRight(sessionId: string, exerciseId: string): Promise<Grade> {
  return call<Grade>('exercise_i_was_right', { session_id: sessionId, exercise_id: exerciseId })
}

/** Check with AI and Try again: asks the AI judge about an unchecked answer. */
export function exerciseCheck(sessionId: string, exerciseId: string): Promise<Grade> {
  return call<Grade>('exercise_check', { session_id: sessionId, exercise_id: exerciseId })
}

/** Explain: why an answer was almost right or wrong. */
export function explainAnswer(sessionId: string, exerciseId: string): Promise<Explained> {
  return call<Explained>('explain_answer', { session_id: sessionId, exercise_id: exerciseId })
}

export function lessonFinish(sessionId: string): Promise<LessonSummary> {
  return call<LessonSummary>('lesson_finish', { session_id: sessionId })
}

export function statsGet(lang: string): Promise<Stats> {
  return call<Stats>('stats_get', { lang })
}

export function backupExport(destPath: string): Promise<ExportSummary> {
  return call<ExportSummary>('backup_export', { dest_path: destPath })
}

export function backupImport(srcPath: string): Promise<ImportReport> {
  return call<ImportReport>('backup_import', { src_path: srcPath })
}

/** The AI status; `refresh` is true when Settings > AI opens (one wallet refresh on publik API). */
export function aiStatus(refresh: boolean): Promise<AiStatus> {
  return call<AiStatus>('ai_status', { refresh })
}

/** The learner said yes to publik API: provisions this computer when it must. */
export function aiConsentAccept(): Promise<AiStatus> {
  return call<AiStatus>('ai_consent_accept')
}

/** Not now: sends nothing, changes nothing. */
export function aiConsentDecline(): Promise<AiStatus> {
  return call<AiStatus>('ai_consent_decline')
}

export function aiSetOwnKey(input: OwnKeyInput): Promise<AiStatus> {
  return call<AiStatus>('ai_set_own_key', {
    base_url: input.base_url,
    key: input.key,
    chat_model: input.chat_model,
    transcribe_model: input.transcribe_model,
    speech_model: input.speech_model,
    speech_voice: input.speech_voice,
  })
}

export function aiClearOwnKey(): Promise<AiStatus> {
  return call<AiStatus>('ai_clear_own_key')
}

/** The models a local server offers, largest first. */
export function aiLocalProbe(baseUrl: string): Promise<LocalModel[]> {
  return call<LocalModel[]>('ai_local_probe', { base_url: baseUrl })
}

/** Chooses the local server and a model, or none (the largest it offers). */
export function aiSetLocal(baseUrl: string, chatModel: string | null): Promise<AiStatus> {
  return call<AiStatus>('ai_set_local', { base_url: baseUrl, chat_model: chatModel })
}

export function aiCheckConnection(): Promise<ConnectionCheck> {
  return call<ConnectionCheck>('ai_check_connection')
}

export function aiDisconnectPublik(): Promise<AiStatus> {
  return call<AiStatus>('ai_disconnect_publik')
}

export function aiReconnectPublik(): Promise<AiStatus> {
  return call<AiStatus>('ai_reconnect_publik')
}

/** Opens the claim, top-up or add-credit link in the browser. */
export function aiOpenLink(kind: AiLink): Promise<null> {
  return call<null>('ai_open_link', { kind })
}

/** The installed OS voices of a language, then the natural voice when natural voices are on. */
export function voicesList(lang: string): Promise<Voice[]> {
  return call<Voice[]>('voices_list', { lang })
}

/**
 * Reads a sentence aloud; resolves once it has started. `audio://ended` follows when it ends.
 * `sessionId` is null outside a lesson.
 */
export function ttsPlay(
  text: string,
  lang: string,
  slow: boolean,
  sessionId: string | null,
): Promise<TtsPlayed> {
  return call<TtsPlayed>('tts_play', { text, lang, slow, session_id: sessionId })
}

export function ttsStop(): Promise<null> {
  return call<null>('tts_stop', {})
}

/** Fetches the next exercise's sentence into the audio cache when natural voices are on. */
export function ttsPrefetch(sessionId: string, exerciseId: string): Promise<Prefetched> {
  return call<Prefetched>('tts_prefetch', { session_id: sessionId, exercise_id: exerciseId })
}

export function audioStatus(): Promise<AudioStatus> {
  return call<AudioStatus>('audio_status', {})
}

/** Can't listen now (`listen`) or Can't speak now (`speak`): off for 60 minutes. */
export function sessionSkip(kind: 'listen' | 'speak'): Promise<null> {
  return call<null>('session_skip', { kind })
}

/**
 * Starts a take of at most `maxS` seconds (15 for an exercise). `mic://level` follows every 50 ms
 * and `mic://limit` once at the limit.
 */
export function micStart(maxS: 15 | 30): Promise<null> {
  return call<null>('mic_start', { max_s: maxS })
}

/** Stops the take; it is kept on this computer and sent nowhere. */
export function micStop(): Promise<Recording> {
  return call<Recording>('mic_stop', {})
}

/** Replay my take. */
export function recordingPlay(recordingId: string): Promise<null> {
  return call<null>('recording_play', { recording_id: recordingId })
}

/** Check: transcribes the take and scores it; `local` is Use the local model. */
export function speakCheck(
  sessionId: string,
  exerciseId: string,
  recordingId: string,
  local: boolean,
): Promise<SpeakResult> {
  return call<SpeakResult>('speak_check', {
    session_id: sessionId,
    exercise_id: exerciseId,
    recording_id: recordingId,
    local,
  })
}

export function whisperModels(): Promise<ModelInfo[]> {
  return call<ModelInfo[]>('whisper_models', {})
}

/** Downloads a local Whisper model; progress arrives on `model://progress` (see onModelProgress). */
export function whisperDownload(name: string): Promise<ModelInfo[]> {
  return call<ModelInfo[]>('whisper_download', { name })
}

export function speakingStatus(): Promise<SpeakingStatus> {
  return call<SpeakingStatus>('speaking_status', {})
}

// Events -----------------------------------------------------------------------------------------

/**
 * Listens to download progress until the returned function is called. Called only while a pack
 * download runs; where events are not available the handler is simply never called.
 */
export async function onPackProgress(
  handler: (progress: PackProgress) => void,
): Promise<() => void> {
  try {
    const unlisten = await listen<PackProgress>(PACK_PROGRESS_EVENT, (event) =>
      handler(event.payload),
    )
    return unlisten
  } catch {
    return () => {}
  }
}

/**
 * Listens for the end of a clip or of the OS voice until the returned function is called. Where
 * events are not available the handler is simply never called.
 */
export async function onAudioEnded(handler: () => void): Promise<() => void> {
  try {
    const unlisten = await listen<null>(AUDIO_ENDED_EVENT, () => handler())
    return typeof unlisten === 'function' ? unlisten : () => {}
  } catch {
    return () => {}
  }
}

/**
 * Listens to the microphone's level while a take is recorded, until the returned function is
 * called. Where events are not available the handler is simply never called.
 */
export async function onMicLevel(handler: (level: MicLevel) => void): Promise<() => void> {
  try {
    const unlisten = await listen<MicLevel>(MIC_LEVEL_EVENT, (event) => handler(event.payload))
    return typeof unlisten === 'function' ? unlisten : () => {}
  } catch {
    return () => {}
  }
}

/**
 * Listens for a take reaching its limit, until the returned function is called. Where events are
 * not available the handler is simply never called.
 */
export async function onMicLimit(handler: () => void): Promise<() => void> {
  try {
    const unlisten = await listen<unknown>(MIC_LIMIT_EVENT, () => handler())
    return typeof unlisten === 'function' ? unlisten : () => {}
  } catch {
    return () => {}
  }
}

/**
 * Listens to a model download's progress until the returned function is called. Where events are
 * not available the handler is simply never called.
 */
export async function onModelProgress(
  handler: (progress: ModelProgress) => void,
): Promise<() => void> {
  try {
    const unlisten = await listen<ModelProgress>(MODEL_PROGRESS_EVENT, (event) =>
      handler(event.payload),
    )
    return typeof unlisten === 'function' ? unlisten : () => {}
  } catch {
    return () => {}
  }
}

/**
 * Listens to AI status changes until the returned function is called. Where events are not
 * available the handler is simply never called.
 */
export async function onAiStatus(handler: (status: AiStatus) => void): Promise<() => void> {
  try {
    const unlisten = await listen<AiStatus>(AI_STATUS_EVENT, (event) => handler(event.payload))
    return typeof unlisten === 'function' ? unlisten : () => {}
  } catch {
    return () => {}
  }
}
