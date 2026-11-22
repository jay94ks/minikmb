export type LogLevel = 'debug' | 'info' | 'warn' | 'error'
export type LogFields = Record<string, string | number | boolean | null | undefined>
export type OperationLog = (level: LogLevel, event: string, fields?: LogFields) => void
export interface LogEntry { time: string; level: LogLevel; event: string; fields: LogFields }
export interface LogState {
  enabled: boolean; verbose: boolean; directory: string; file: string
  dropped: number; writeError: string; recent: LogEntry[]
}
