import { chordUsages, type KeyChordRequest, type SpecialInputState } from '../src/shared/special-input'
import type { OperationLog } from '../src/shared/logging'
import { systemControl } from '../src/shared/system-controls'
export class SystemControlRejected extends Error {
  constructor(readonly status: 'unsupported' | 'not-ready', message: string) { super(message) }
}
type Port = 0 | 1
type Release = { op: number; usage: number }
// Main owns the whole sequence, including cleanup after renderer focus loss.
// DOWN is attempted once; even a lost DOWN ACK still requires an UP on that port.
export class SpecialInput {
  private jobs: Array<Promise<void> | undefined> = [undefined, undefined]
  private releases: [Release[], Release[]] = [[], []]
  private canceled = [false, false]
  private rebooted = [false, false]
  private closing = false
  constructor(private send: (port: Port, op: number, payload: Uint8Array) => Promise<void>,
    private available: (port: Port) => boolean, private log: OperationLog = () => {},
    private wait = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))) {}
  state(): [SpecialInputState, SpecialInputState] {
    return [0, 1].map(port => ({ busy: Boolean(this.jobs[port]), needsRelease: this.releases[port]!.length > 0 })) as [SpecialInputState, SpecialInputState]
  }
  blocked(port: number): boolean { return Boolean(this.jobs[port] || this.releases[port]?.length) }
  restarted(port: Port): void {
    this.canceled[port] = true
    this.rebooted[port] = true
    this.releases[port] = [] // New slave boot has no old held keys to recover.
    this.log('warn', 'input.slave_restarted', { port })
  }
  chord(port: Port, request: KeyChordRequest): Promise<void> {
    return this.run(port, chordUsages(request), 0x10, 0x11)
  }
  power(port: Port, confirmed: boolean): Promise<void> {
    return this.system(port, 0x81, confirmed)
  }
  system(port: Port, usage: number, confirmed: boolean): Promise<void> {
    const control = systemControl(usage)
    if (control.confirm && confirmed !== true) return Promise.reject(new Error('전원/시스템 버튼 전송을 확인해야 합니다.'))
    return this.run(port, [usage], 0x12, 0x13)
  }
  private validate(port: Port): void {
    if ((port !== 0 && port !== 1) || !this.available(port)) throw new Error('연결되어 입력을 사용할 수 있는 대상이 필요합니다.')
    if (this.jobs[port]) throw new Error('해당 대상의 특수 입력이 진행 중입니다.')
  }
  private run(port: Port, keys: number[], down: number, up: number): Promise<void> {
    this.validate(port)
    if (this.closing || this.releases[port].length) throw new Error('미확인 입력을 먼저 해제하세요.')
    this.canceled[port] = false
    this.rebooted[port] = false
    const kind = down === 0x12 ? (keys[0] === 0x81 ? 'power' : 'system') : 'sequence'
    const job = (async () => {
      let failure: unknown
      this.log('info', `input.${kind}_started`, { port, count: keys.length, system: down === 0x12 })
      try {
        for (const usage of keys) {
          if (this.canceled[port]) throw new Error('특수 입력이 취소되었습니다.')
          const release = { op: up, usage }
          this.releases[port].push(release) // BEFORE awaiting possibly lost ACK.
          try {
            await this.send(port, down, Uint8Array.of(usage))
            if (down === 0x12) this.log('info', `input.${kind}_down_accepted`, { port, hostActionConfirmed: false })
          } catch (error) {
            // An explicit negative slave ACK proves this DOWN was not queued.
            // A timeout/transport failure never provides that guarantee.
            if (error instanceof SystemControlRejected) this.releases[port].splice(this.releases[port].indexOf(release), 1)
            if (down === 0x12) this.log('error', `input.${kind}_down_failed`, { port, hostActionConfirmed: false, error: error instanceof Error ? error.message : String(error) })
            throw error
          }
        }
        await this.wait(100)
        if (this.canceled[port]) throw new Error('slave 재부팅으로 이전 입력이 취소되었습니다.')
      } catch (error) { failure = error }
      finally {
        for (const release of [...this.releases[port]].reverse()) {
          if (this.rebooted[port]) break
          try {
            await this.send(port, release.op, Uint8Array.of(release.usage))
            if (release.op === 0x13) this.log('info', `input.${kind}_up_accepted`, { port, hostActionConfirmed: false })
            this.releases[port].splice(this.releases[port].indexOf(release), 1)
          } catch (error) {
            if (release.op === 0x13) this.log('error', `input.${kind}_up_failed`, { port, needsRelease: true, error: error instanceof Error ? error.message : String(error) })
            failure ??= error
          } // Attempt EVERY UP, not only the first.
        }
      }
      this.log(failure ? 'error' : 'info', `input.${kind}_finished`, {
        port, success: !failure, needsRelease: this.releases[port].length > 0,
        error: failure instanceof Error ? failure.message : failure ? String(failure) : undefined })
      if (failure) throw failure
    })()
    this.jobs[port] = job
    void job.finally(() => { this.jobs[port] = undefined }).catch(() => {})
    return job
  }
  async release(port: Port): Promise<void> {
    this.validate(port)
    let failure: unknown
    const job = (async () => {
      for (const release of [...this.releases[port]].reverse()) {
        try {
          await this.send(port, release.op, Uint8Array.of(release.usage))
          if (release.op === 0x13) this.log('info', release.usage === 0x81 ? 'input.power_up_accepted' : 'input.system_up_accepted', { port, recovery: true, hostActionConfirmed: false })
          this.releases[port].splice(this.releases[port].indexOf(release), 1)
        } catch (error) { failure ??= error }
      }
      try { await this.send(port, 0x7f, new Uint8Array()); this.releases[port] = []; failure = undefined }
      catch (error) { failure ??= error }
      this.log(failure ? 'error' : 'info', 'input.release_all', { port, success: !failure })
      if (failure) throw failure
    })()
    this.jobs[port] = job
    try { await job } finally { this.jobs[port] = undefined }
  }
  async cancel(port: Port): Promise<void> {
    this.canceled[port] = true
    await this.jobs[port]?.catch(() => {})
    if (this.releases[port].length && this.available(port)) await this.release(port)
  }
  async stop(): Promise<void> {
    this.closing = true; this.canceled = [true, true]
    await Promise.allSettled(this.jobs.filter((job): job is Promise<void> => Boolean(job)))
    for (const port of [0, 1] as const) if (this.releases[port].length && this.available(port)) {
      await this.release(port).catch(() => {})
    }
  }
}
