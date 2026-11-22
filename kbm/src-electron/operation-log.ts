import fs from 'node:fs/promises'
import path from 'node:path'
import type { LogEntry, LogFields, LogLevel, LogState, OperationLog } from '../src/shared/logging'

// Bounded JSONL logging. Never accept packet bodies, key values, file content,
// URLs or source paths. Only explicitly supplied operation metadata is stored.
export class OperationLogger {
  private enabled = true
  private verbose = false
  private recent: LogEntry[] = []
  private queue: string[] = []
  private dropped = 0
  private writeError = ''
  private timer: ReturnType<typeof setTimeout> | undefined
  private writing: Promise<void> | undefined
  private size = -1
  readonly file: string
  constructor(readonly directory: string, private maxBytes = 4 * 1024 * 1024, private files = 5) {
    this.file = path.join(directory, 'operations.jsonl')
  }
  readonly log: OperationLog = (level, event, fields = {}) => {
    if (!this.enabled || (level === 'debug' && !this.verbose)) return
    const safe: LogFields = {}
    for (const [key, value] of Object.entries(fields).slice(0, 24)) {
      if (/payload|content|password|secret|token|path|filename|usage|keycode/i.test(key)) continue
      if (typeof value === 'string') safe[key] = value.replace(/[\r\n\x00-\x1f]/g, ' ')
        .replace(/[a-zA-Z]:[\\/].*|file:\/\/\S+/g, '[redacted-path]').slice(0, 256)
      else if (typeof value === 'boolean' || value === null || (typeof value === 'number' && Number.isFinite(value))) safe[key] = value
    }
    const entry: LogEntry = { time: new Date().toISOString(), level,
      event: event.replace(/[^a-zA-Z0-9_.:-]/g, '_').slice(0, 80), fields: safe }
    this.recent.push(entry); if (this.recent.length > 300) this.recent.shift()
    if (this.queue.length >= 1024) { ++this.dropped; return }
    this.queue.push(JSON.stringify(entry) + '\n')
    if (!this.timer) this.timer = setTimeout(() => { this.timer = undefined; void this.flush() }, 250)
  }
  snapshot(): LogState {
    return { enabled: this.enabled, verbose: this.verbose, directory: this.directory, file: this.file,
      dropped: this.dropped, writeError: this.writeError, recent: structuredClone(this.recent) }
  }
  async configure(enabled: boolean, verbose: boolean): Promise<void> {
    this.log('info', 'logging.configure', { enabled, verbose })
    this.enabled = enabled; this.verbose = verbose
    this.log('info', 'logging.enabled', { enabled, verbose })
    await this.flush()
  }
  async flush(): Promise<void> {
    if (this.timer) { clearTimeout(this.timer); this.timer = undefined }
    if (this.writing) { await this.writing; if (this.queue.length) await this.flush(); return }
    if (!this.queue.length) return
    const pending = this.queue.splice(0)
    this.writing = (async () => {
      try {
        await fs.mkdir(this.directory, { recursive: true })
        if (this.size < 0) this.size = await fs.stat(this.file).then(stat => stat.size, () => 0)
        let batch = ''
        const append = async (): Promise<void> => { if (batch) { await fs.appendFile(this.file, batch); batch = '' } }
        for (const line of pending) {
          const bytes = Buffer.byteLength(line)
          if (this.size && this.size + bytes > this.maxBytes) {
            await append()
            for (let n = this.files - 1; n >= 1; --n) {
              const from = n === 1 ? this.file : `${this.file}.${n - 1}`
              const to = `${this.file}.${n}`
              await fs.rename(from, to).catch(error => { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error })
            }
            this.size = 0
          }
          batch += line
          this.size += bytes
          if (Buffer.byteLength(batch) >= 65536) await append()
        }
        await append()
        this.writeError = ''
      } catch (error) {
        this.writeError = error instanceof Error ? error.message : String(error)
        this.dropped += pending.length // Never let logging failure break USB/control.
      }
    })()
    await this.writing; this.writing = undefined
  }
}
