import { encodePacket, Op, type Packet } from '../src/services/protocol'
import type { OperationLog } from '../src/shared/logging'
type Port = 0 | 1
interface Request {
  packet: Packet; current: () => boolean
  resolve: (response: Packet) => void; reject: (error: Error) => void
  wireId?: number; timer?: ReturnType<typeof setTimeout>
  started?: number
}
// Single UART arbiter shared by renderer control and native disk serving.
// One real ACK at a time per port; control wins at every 64-byte bulk boundary.
export class RoutedTransport {
  private control: Request[][] = [[], []]
  private bulk: Request[][] = [[], []]
  private active: Array<Request | undefined> = [undefined, undefined]
  private sequence = 0
  constructor(private write: (bytes: Uint8Array) => Promise<void>, private log: OperationLog = () => {}) {}
  request(packet: Packet, bulk = false, current: () => boolean = () => true): Promise<Packet> {
    const port = packet.payload[0]
    if (packet.op !== Op.Route || packet.payload.length < 4 || (port !== 0 && port !== 1)) {
      return Promise.reject(new Error('Invalid addressed transport request'))
    }
    const queue = (bulk ? this.bulk : this.control)[port]!
    if (queue.length >= (bulk ? 16 : 64)) {
      this.log('warn', 'uart.queue_full', { port, bulk, queued: queue.length })
      return Promise.reject(new Error('UART transport queue full'))
    }
    return new Promise((resolve, reject) => {
      queue.push({ packet, current, resolve, reject }); this.pump(port)
    })
  }
  accept(packet: Packet): boolean {
    if (packet.op !== (Op.Route | 128) || packet.payload.length < 5) return false
    const p = packet.payload, port = p[0]
    if (port !== 0 && port !== 1) return true
    const active = this.active[port], id = p[1]! | p[2]! << 8
    if (!active || id !== active.wireId) return true // Late ACK cannot consume another request.
    if (p[3] === 0 && (p[4] !== (active.packet.payload[3]! | 128) || p.length < 7)) return true
    const restored = p.slice()
    restored[1] = active.packet.payload[1]!; restored[2] = active.packet.payload[2]!
    this.finish(port, active, { op: packet.op, payload: restored })
    return true
  }
  reset(): void {
    for (const port of [0, 1] as const) {
      this.resetPort(port, 'Master disconnected')
    }
    // Do not reuse IDs at reconnect; late responses stay harmless.
  }
  resetPort(port: Port, reason = 'Slave restarted; old requests canceled'): void {
      const active = this.active[port]
      this.active[port] = undefined
      if (active?.timer) clearTimeout(active.timer)
      active?.reject(new Error(reason))
      for (const queue of [this.control[port]!, this.bulk[port]!]) {
        for (const request of queue.splice(0)) request.reject(new Error(reason))
      }
  }
  private pump(port: Port): void {
    if (this.active[port]) return
    let next: Request | undefined
    while ((next = this.control[port]!.shift() ?? this.bulk[port]!.shift())) {
      if (next.current()) break
      next.reject(new Error('Canceled disk transfer')); next = undefined
    }
    if (!next) return
    const request = next, payload = request.packet.payload.slice()
    request.wireId = this.sequence = (this.sequence + 1) & 65535
    payload[1] = request.wireId & 255; payload[2] = request.wireId >>> 8
    this.active[port] = request
    request.started = performance.now()
    this.log('debug', 'uart.request', { port, op: payload[3], id: request.wireId, length: payload.length - 4 })
    request.timer = setTimeout(() => this.finish(port, request, undefined, new Error(`UART${port} transport timeout`)), 1100)
    void this.write(encodePacket({ op: Op.Route, payload })).catch(error => {
      this.finish(port, request, undefined, error instanceof Error ? error : new Error(String(error)))
    })
  }
  private finish(port: Port, request: Request, response?: Packet, error?: Error): void {
    if (this.active[port] !== request) return
    if (request.timer) clearTimeout(request.timer)
    this.active[port] = undefined
    const fields = { port, op: request.packet.payload[3], id: request.wireId,
      ms: Math.round(performance.now() - (request.started ?? performance.now())),
      bridgeStatus: response?.payload[3], slaveStatus: response?.payload[5], error: error?.message }
    this.log(error ? 'error' : response && (response.payload[3] || response.payload[5]) ? 'warn' : 'debug',
      error ? 'uart.failure' : 'uart.response', fields)
    if (error) request.reject(error)
    else if (response) request.resolve(response)
    queueMicrotask(() => this.pump(port))
  }
}
