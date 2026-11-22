import type { UartPort } from './master-serial'
export interface NetworkObservation { available: boolean; requested: boolean; ready: boolean }
// Desired state survives USB re-enumeration/UART reconnect, never application restart.
export class NetworkSession {
  readonly desired: [boolean, boolean] = [false, false]
  private local: [boolean, boolean] = [false, false]
  private generations = [0, 0]
  private tasks: Promise<void>[] = [Promise.resolve(), Promise.resolve()]
  private lastEnable = [0, 0]
  suspended = false
  reportLocal(port: UartPort, enabled: boolean): void { this.local[port] = enabled }
  constructor(private remote: (port: UartPort, enabled: boolean) => Promise<unknown>,
    private enableLocal: (port: UartPort, enabled: boolean) => Promise<unknown>) {}
  setDesired(port: UartPort, enabled: boolean): Promise<void> {
    this.desired[port] = enabled; ++this.generations[port]!; this.lastEnable[port] = 0
    return this.serial(port, async () => {
      if (!enabled) { await this.setLocal(port, false); if (!this.suspended) await this.remote(port, false) }
    })
  }
  async suspend(): Promise<void> {
    this.suspended = true
    for (const port of [0, 1] as const) { this.desired[port] = false; ++this.generations[port]! }
    await Promise.all(([0, 1] as const).map(port => this.serial(port, () => this.setLocal(port, false))))
  }
  observe(port: UartPort, state: NetworkObservation): Promise<void> {
    const generation = this.generations[port]
    return this.serial(port, async () => {
      if (generation !== this.generations[port]) return
      if (this.suspended || !this.desired[port] || !state.available) {
        if (!state.available) this.lastEnable[port] = 0
        await this.setLocal(port, false); return
      }
      if (!state.requested && Date.now() - this.lastEnable[port]! >= 1500) {
        this.lastEnable[port] = Date.now()
        await this.remote(port, true)
      }
      // Enabling a hidden USB device must precede starting DHCP. Never assume ACK=USB ready.
      if (generation !== this.generations[port] || this.suspended) return
      await this.setLocal(port, state.requested && state.ready)
    })
  }
  private async setLocal(port: UartPort, enabled: boolean): Promise<void> {
    if (this.local[port] === enabled) return
    await this.enableLocal(port, enabled); this.local[port] = enabled
  }
  private serial(port: UartPort, task: () => Promise<void>): Promise<void> {
    const next = this.tasks[port]!.catch(() => {}).then(task)
    this.tasks[port] = next
    return next
  }
}
