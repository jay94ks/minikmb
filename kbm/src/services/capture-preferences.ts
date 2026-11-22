export interface FixedCaptureMode { kind: 'fixed'; width: number; height: number; frameRate: number }
export type CaptureMode = { kind: 'auto' } | FixedCaptureMode
export type CaptureAssignments = [string | null, string | null]
export type CaptureUartPort = 0 | 1
export type CaptureUartPorts = Record<string, CaptureUartPort>
export type ScreenUartPorts = [CaptureUartPort, CaptureUartPort]
interface CapturePreferences {
  assignments: CaptureAssignments
  modes: Record<string, CaptureMode>
  uartPorts: CaptureUartPorts
}
const STORAGE_KEY = 'kbm.capture-preferences.v1'

export function validCaptureMode(value: unknown): value is CaptureMode {
  if (!value || typeof value !== 'object') return false
  const mode = value as Partial<FixedCaptureMode> | { kind?: unknown }
  if (mode.kind === 'auto') return true
  if (mode.kind !== 'fixed') return false
  const fixed = mode as Partial<FixedCaptureMode>
  return Number.isInteger(fixed.width) && Number.isInteger(fixed.height) &&
    fixed.width! >= 2 && fixed.width! <= 8192 && fixed.height! >= 2 && fixed.height! <= 8192 &&
    typeof fixed.frameRate === 'number' && Number.isFinite(fixed.frameRate) &&
    fixed.frameRate >= 1 && fixed.frameRate <= 240
}

export function modeLabel(mode: FixedCaptureMode): string {
  return `${mode.width} x ${mode.height} / ${mode.frameRate} Hz`
}
export function modeKey(mode: FixedCaptureMode): string {
  return `${mode.width}:${mode.height}:${mode.frameRate}`
}

// Candidates, NOT an advertised list of supported driver modes. Only opening the
// exact tuple and receiving its first frame validates it; never combine maxima.
export const CAPTURE_MODE_PRESETS: FixedCaptureMode[] = [
  [3840, 2160, 30], [3840, 2160, 24], [2560, 1440, 60], [2560, 1440, 30],
  [1920, 1080, 60], [1920, 1080, 30], [1920, 1080, 24],
  [1280, 720, 60], [1280, 720, 30], [1280, 720, 24],
  [640, 480, 60], [640, 480, 30], [640, 480, 24]
].map(([width, height, frameRate]) => ({ kind: 'fixed', width: width!, height: height!, frameRate: frameRate! }))

function validDeviceId(id: unknown): id is string {
  return typeof id === 'string' && id.length > 0 && id.length <= 1024
}
export function validAssignments(value: unknown): value is CaptureAssignments {
  return Array.isArray(value) && value.length === 2 &&
    value.every(id => id === null || validDeviceId(id)) &&
    (value[0] === null || value[0] !== value[1])
}
function loadPreferences(): CapturePreferences {
  const defaults: CapturePreferences = { assignments: [null, null], modes: {}, uartPorts: {} }
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '')
    if (!raw || typeof raw !== 'object') return defaults
    const source = raw as Partial<CapturePreferences>
    const modes = Object.fromEntries(Object.entries(source.modes ?? {})
      .filter(([id, mode]) => validDeviceId(id) && validCaptureMode(mode)).slice(-128))
    const uartPorts = Object.fromEntries(Object.entries(source.uartPorts ?? {})
      .filter(([id, port]) => validDeviceId(id) && (port === 0 || port === 1)).slice(-128)) as CaptureUartPorts
    return { assignments: validAssignments(source.assignments) ? [...source.assignments] : [null, null], modes, uartPorts }
  } catch { return defaults }
}
export function loadCaptureAssignments(): CaptureAssignments { return loadPreferences().assignments }
export function saveCaptureAssignments(assignments: CaptureAssignments): void {
  if (!validAssignments(assignments)) throw new Error('각 화면에는 서로 다른 캡처 장치를 배정하세요.')
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...loadPreferences(), assignments }))
}
export function loadCaptureMode(deviceId: string): CaptureMode {
  const modes = loadPreferences().modes
  return Object.hasOwn(modes, deviceId) ? modes[deviceId]! : { kind: 'auto' }
}
export function saveCaptureMode(deviceId: string, mode: CaptureMode): void {
  if (!validDeviceId(deviceId) || !validCaptureMode(mode)) throw new Error('올바르지 않은 영상 모드입니다.')
  const preferences = loadPreferences()
  // Bound storage and make the latest successful setting the newest entry.
  delete preferences.modes[deviceId]
  preferences.modes[deviceId] = mode
  preferences.modes = Object.fromEntries(Object.entries(preferences.modes).slice(-128))
  localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences))
}

export function loadCaptureUartPorts(): CaptureUartPorts { return loadPreferences().uartPorts }

export function captureScreenPorts(
  assignments: CaptureAssignments, devicePorts: CaptureUartPorts, fallback: ScreenUartPorts
): ScreenUartPorts {
  return assignments.map((id, index) => id && Object.hasOwn(devicePorts, id)
    ? devicePorts[id]! : fallback[index]!) as ScreenUartPorts
}

// Migrate the old screen-based choice only for boards without a saved binding.
// This also pins disconnected assigned boards, so later swaps never inherit the
// destination screen's UART. Existing bindings (including UART0) always win.
export function initializeCaptureUartPorts(
  assignments: CaptureAssignments, fallback: ScreenUartPorts
): CaptureUartPorts {
  const preferences = loadPreferences()
  let changed = false
  assignments.forEach((id, index) => {
    if (id && !Object.hasOwn(preferences.uartPorts, id)) {
      preferences.uartPorts[id] = fallback[index]!
      changed = true
    }
  })
  if (changed) {
    preferences.uartPorts = Object.fromEntries(Object.entries(preferences.uartPorts).slice(-128))
    localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences))
  }
  return preferences.uartPorts
}

export function saveCaptureUartPort(deviceId: string, port: CaptureUartPort): CaptureUartPorts {
  if (!validDeviceId(deviceId) || (port !== 0 && port !== 1)) throw new Error('올바르지 않은 캡처보드 UART 배정입니다.')
  const preferences = loadPreferences()
  delete preferences.uartPorts[deviceId]
  preferences.uartPorts[deviceId] = port
  preferences.uartPorts = Object.fromEntries(Object.entries(preferences.uartPorts).slice(-128))
  localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences))
  return preferences.uartPorts
}

export function resolveCaptureAssignments(saved: CaptureAssignments, available: string[]): CaptureAssignments {
  const next: CaptureAssignments = [...saved]
  for (const index of [0, 1] as const) {
    // Missing pinned boards keep their screen; never silently swap targets.
    if (next[index] === null) next[index] = available.find(id => !next.includes(id)) ?? null
  }
  return next
}
