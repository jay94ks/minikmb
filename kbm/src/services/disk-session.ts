import type { DiskDeviceStatus, DiskReadRequest, DiskSnapshotInfo } from '../shared/disk'
import type { UartPort } from './master-serial'

export type DiskPhase = 'hidden' | 'starting' | 'active' | 'stopping' | 'unavailable' | 'ejected' | 'error'
export interface DiskPortState {
  available: boolean; enabled: boolean; busy: boolean; phase: DiskPhase
  device: DiskDeviceStatus | undefined; snapshot: DiskSnapshotInfo | undefined
  reads: number; error: string
}
export interface DiskSessionBackend {
  prepare(port: UartPort): Promise<DiskSnapshotInfo>
  disable(port: UartPort): Promise<void>
  read(port: UartPort, request: DiskReadRequest): Promise<Uint8Array>
  configure(port: UartPort, generation: number, sectors: number, enabled: boolean): Promise<unknown>
  status(port: UartPort): Promise<DiskDeviceStatus>
  data(port: UartPort, request: DiskReadRequest, bytes: Uint8Array, current: () => boolean): Promise<void>
  fail(port: UartPort, request: DiskReadRequest): Promise<unknown>
}
const initial = (): DiskPortState => ({ available: false, enabled: false, busy: false, phase: 'unavailable',
  device: undefined, snapshot: undefined, reads: 0, error: '' })
const message = (error: unknown): string => error instanceof Error ? error.message : String(error)

// Per-port epochs cancel old preparation, reads and heartbeat work. No automatic
// ON/retry after disconnect/eject; ACK means queued, never USB mount completion.
export class DiskSession {
  readonly ports: [DiskPortState, DiskPortState] = [initial(), initial()]
  private epochs = [0, 0]
  private tasks = [Promise.resolve(), Promise.resolve()]
  private polling = [false, false]
  private serving = [false, false]
  private lastHeartbeat = [0, 0]
  private startedAt = [0, 0]
  constructor(private backend: DiskSessionBackend, private changed: () => void = () => {},
    private now: () => number = Date.now) {}

  observe(port: UartPort, available: boolean): void {
    const state = this.ports[port]
    if (state.available === available) return
    state.available = available; state.device = undefined
    if (!available) {
      ++this.epochs[port]!; state.enabled = false; state.busy = false; state.phase = 'unavailable'
      void this.backend.disable(port).catch(error => { state.error = message(error); this.changed() })
    } else state.phase = 'hidden'
    this.changed()
  }
  start(port: UartPort): Promise<void> {
    const state = this.ports[port], epoch = ++this.epochs[port]!
    state.enabled = false; state.busy = true; state.phase = 'starting'; state.error = ''; this.changed()
    return this.serial(port, async () => {
      try {
        if (!state.available) throw new Error(`UART${port}: slave가 오프라인입니다.`)
        if (epoch !== this.epochs[port]) return
        await this.backend.disable(port)
        if (state.snapshot) await this.backend.configure(port, state.snapshot.generation, 0, false)
        if (epoch !== this.epochs[port]) return
        const snapshot = await this.backend.prepare(port)
        if (epoch !== this.epochs[port]) return
        state.snapshot = snapshot; state.enabled = true; state.device = undefined; state.reads = 0
        await this.backend.configure(port, snapshot.generation, snapshot.sectors, true)
        if (epoch !== this.epochs[port]) return
        this.lastHeartbeat[port] = this.startedAt[port] = this.now()
      } catch (error) {
        if (epoch !== this.epochs[port]) return
        state.enabled = false; state.phase = 'error'; state.error = message(error)
        await this.backend.disable(port).catch(() => {})
        // A lost CONFIG ACK may still have enabled the remote. No ON retry;
        // without heartbeat its USB storage disappears within five seconds.
      } finally {
        if (epoch === this.epochs[port]) state.busy = false
        this.changed()
      }
    })
  }
  stop(port: UartPort): Promise<void> {
    const state = this.ports[port], epoch = ++this.epochs[port]!
    state.enabled = false; state.busy = true; state.phase = 'stopping'; state.error = ''; this.changed()
    // Cancel a native prepare immediately, even while the serialized task waits.
    const disabled = this.backend.disable(port).then(() => undefined, error => ({ error }))
    return this.serial(port, async () => {
      try {
        const failure = await disabled
        if (failure) throw failure.error
        if (epoch !== this.epochs[port]) return
        if (state.available && state.snapshot) await this.backend.configure(port, state.snapshot.generation, 0, false)
        if (!state.available) state.phase = 'unavailable'
      } catch (error) {
        if (epoch === this.epochs[port]) { state.phase = 'error'; state.error = message(error) }
      } finally {
        if (epoch === this.epochs[port]) state.busy = false
        this.changed()
      }
    })
  }
  async tick(port: UartPort): Promise<void> {
    const state = this.ports[port]
    if (this.polling[port] || state.busy || !state.available) return
    this.polling[port] = true
    const epoch = this.epochs[port]
    try {
      await this.serial(port, async () => {
        if (epoch !== this.epochs[port] || !state.available) return
        if (state.enabled && state.snapshot && this.now() - this.lastHeartbeat[port]! >= 2000) {
          await this.backend.configure(port, state.snapshot.generation, state.snapshot.sectors, true)
          this.lastHeartbeat[port] = this.now()
        }
        if (epoch !== this.epochs[port]) return
        const status = await this.backend.status(port)
        if (epoch !== this.epochs[port]) return
        state.device = status
        if (!state.enabled) {
          if (state.phase !== 'ejected' && state.phase !== 'error') state.phase = status.ready ? 'stopping' : 'hidden'
        } else if (status.ready) state.phase = 'active'
        else if (this.now() - this.startedAt[port]! >= 1000) {
          state.enabled = false
          state.phase = status.configured ? 'ejected' : 'error'
          state.error = status.configured ? '꺼내기 또는 공유 lease 만료 감지. 새로고침으로 다시 연결하세요.' : 'Slave가 공유 설정을 잃었습니다. 공유를 다시 켜세요.'
          await this.backend.disable(port)
        }
      })
    } catch (error) {
      if (epoch === this.epochs[port]) {
        state.enabled = false; state.phase = 'error'; state.device = undefined; state.error = message(error)
        await this.backend.disable(port).catch(() => {})
      }
    } finally { this.polling[port] = false; this.changed() }
  }
  async serve(port: UartPort, request: DiskReadRequest): Promise<void> {
    const state = this.ports[port], snapshot = state.snapshot, epoch = this.epochs[port]
    if (!state.enabled || !snapshot || request.generation !== snapshot.generation || this.serving[port]) return
    this.serving[port] = true
    const current = (): boolean => state.enabled && epoch === this.epochs[port] && state.snapshot === snapshot
    try {
      const bytes = await this.backend.read(port, request)
      if (!current()) return
      await this.backend.data(port, request, bytes, current)
      if (current()) ++state.reads
    } catch (error) {
      if (current()) {
        state.error = message(error)
        await this.backend.fail(port, request).catch(() => {})
      }
    } finally { this.serving[port] = false; this.changed() }
  }
  private serial(port: UartPort, task: () => Promise<void>): Promise<void> {
    const next = this.tasks[port]!.catch(() => {}).then(task)
    this.tasks[port] = next
    return next
  }
}
