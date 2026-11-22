import net from 'node:net'
import dgram from 'node:dgram'
import { createStack, type NetworkStack, type TapInterface, type TcpConnection, type UdpSocket } from 'tcpip'
import { defaultNetworkConfig, validateNetworkConfig, validateForwardRule,
  type NetworkConfig, type NetworkPort, type ForwardRule, type NetworkState } from '../src/shared/network'
import { DhcpServer } from './dhcp'
import { ensureWasmStreaming } from './wasm-runtime'
import type { OperationLog } from '../src/shared/logging'
import { HostGateway } from './host-gateway'
import { PortScanner, emptyScan } from './port-scanner'
import type { PortScanRequest, PortScanState } from '../src/shared/network'

interface Link {
  stack: NetworkStack
  tap: TapInterface
  writer: WritableStreamDefaultWriter<Uint8Array>
  reader: ReadableStreamDefaultReader<Uint8Array> | undefined
  dhcp: DhcpServer
  tx: Uint8Array[]
  rx: Uint8Array[]
  pumping: boolean
  receiving: boolean
  epoch: number
  scanner: PortScanner
}
interface ForwardHandle { port: NetworkPort; close(): void }
// The app is a pair of isolated /24 networks, not a bridge between slaves.
// Node listeners are NEW loopback entry points; no existing PC service is exposed.
export class NetworkService {
  private config: NetworkConfig = { ...defaultNetworkConfig }
  private stacks: Array<NetworkStack | undefined> = [undefined, undefined]
  private links: Array<Link | undefined> = [undefined, undefined]
  private gateways: Array<HostGateway | undefined> = [undefined, undefined]
  private handles: ForwardHandle[] = []
  private closers = new Map<() => void, NetworkPort>()
  private state: NetworkState = { enabled: [false, false], forwarding: false,
    rxFrames: [0, 0], txFrames: [0, 0], dropped: [0, 0], queued: [0, 0], leases: [], connections: 0, error: '' }
  constructor(private send: (port: NetworkPort, bytes: Uint8Array) => Promise<void>, private log: OperationLog = () => {}) {}
  snapshot(): NetworkState {
    this.state.connections = this.closers.size + this.gateways.reduce((n, g) => n + (g?.count ?? 0), 0)
    this.state.queued = [this.links[0]?.tx.length ?? 0, this.links[1]?.tx.length ?? 0]
    this.state.leases = this.links.flatMap((link, port) => link ?
      Array.from(link.dhcp.leases, ([mac, lease]) => ({ port: port as NetworkPort, ip: lease.ip, mac })) : [])
    return structuredClone(this.state)
  }
  async configure(config: NetworkConfig): Promise<void> {
    validateNetworkConfig(config)
    if (this.state.enabled.some(Boolean)) throw new Error('설정을 변경하려면 네트워크를 먼저 끄세요.')
    this.config = { serverIp: config.serverIp, poolStart: config.poolStart,
      poolEnd: config.poolEnd, leaseSeconds: config.leaseSeconds }
  }
  isSharing(port: NetworkPort): boolean { return Boolean(this.gateways[port]?.active && this.links[port]) }
  scanState(port: NetworkPort): PortScanState { return this.links[port]?.scanner.state() ?? emptyScan() }
  scan(port: NetworkPort, request: PortScanRequest | null): void {
    const link = this.links[port]
    if (!link) throw new Error('NCM을 먼저 켜세요.')
    if (request === null) link.scanner.stop(); else link.scanner.start(request)
  }
  async enable(port: NetworkPort, enabled: boolean, sharing = false): Promise<void> {
    if (port !== 0 && port !== 1) throw new Error('Invalid UART')
    if (this.state.enabled[port] === enabled) return
    this.log('info', 'network.enable', { port, enabled })
    if (!enabled) {
      this.stopForwarding(port)
      this.state.enabled[port] = false
      const link = this.links[port]
      this.links[port] = undefined
      if (link) {
        link.scanner.stop(); this.gateways[port]?.stop()
        ++link.epoch; link.tx.length = link.rx.length = 0
        // tcpip0.4 can enqueue a deferred lwIP output after reader.cancel().
        // Unregister the tap handle FIRST, while its stream is still open, so
        // late callbacks cannot enqueue into a closed controller and crash main.
        await link.stack.interfaces.remove(link.tap)
        await link.reader?.cancel().catch(() => {})
        await link.writer.close().catch(() => {})
      }
      return
    }
    ensureWasmStreaming()
    const stack = this.stacks[port] ?? await createStack({ initializeLoopback: false })
    this.stacks[port] = stack // Reuse at most two WASM instances; package has no dispose().
    const mac = Uint8Array.of(2, 0x4b, 0x42, 0x4d, 0, port + 1)
    const tap = await stack.interfaces.createTap({ ip: `${this.config.serverIp}/24` as `${number}.${number}.${number}.${number}/${number}`,
      mac: Array.from(mac, n => n.toString(16).padStart(2, '0')).join(':') as `${string}:${string}:${string}:${string}:${string}:${string}` })
    const link: Link = { stack, tap, writer: tap.writable.getWriter(), reader: undefined, tx: [], rx: [],
      pumping: false, receiving: false, epoch: 0,
      dhcp: new DhcpServer(this.config, mac, this.config.poolStart + port, sharing ? this.config.serverIp : undefined),
      scanner: new PortScanner(this.config.serverIp, mac, frame => this.enqueue(port, link, frame)) }
    if (sharing) {
      const gateway = this.gateways[port] ?? new HostGateway(stack, () => this.closers.size + (this.gateways[port]?.count ?? 0) < 32, () => { ++this.state.dropped[port] }, undefined, e => this.fail(e))
      this.gateways[port] = gateway
      try { await gateway.start(this.config.serverIp) } catch (e) { gateway.stop(); await stack.interfaces.remove(tap); await link.writer.close().catch(() => {}); throw e }
    }
    this.links[port] = link
    this.state.enabled[port] = true; this.state.error = ''
    // Drain lwIP's unbounded raw readable immediately into OUR bounded queue.
    void (async () => {
      const reader = tap.readable.getReader()
      link.reader = reader
      try {
        while (this.links[port] === link) {
          const item = await reader.read()
          if (item.done) break
          if (this.links[port] === link) this.enqueue(port, link, item.value)
        }
      } catch (error) { if (this.links[port] === link) this.fail(error) }
      finally { void reader.cancel().catch(() => {}); reader.releaseLock() }
    })()
  }
  receive(port: NetworkPort, bytes: unknown): void {
    const link = this.links[port]
    if (!link || !Array.isArray(bytes) || bytes.length < 14 || bytes.length > 1514 ||
        bytes.some(n => !Number.isInteger(n) || n < 0 || n > 255)) return
    if (link.rx.length >= 32) { ++this.state.dropped[port]; return }
    link.rx.push(Uint8Array.from(bytes))
    if (!link.receiving) void this.receiveQueued(port, link)
  }
  private async receiveQueued(port: NetworkPort, link: Link): Promise<void> {
    link.receiving = true
    try {
      while (this.links[port] === link && link.rx.length) {
        const frame = link.rx.shift()!
        ++this.state.rxFrames[port]
        const dhcp = link.dhcp.handle(frame)
        if (dhcp) this.enqueue(port, link, dhcp)
        if (link.scanner.receive(frame)) continue
        // Only ARP and IPv4. No IPv6, discovery daemons, or higher-level services.
        if (frame[12] === 8 && (frame[13] === 0 || frame[13] === 6)) {
          const delivered = this.gateways[port] ? await this.gateways[port]!.ingress(frame) : frame
          if (this.links[port] === link && delivered) await link.writer.write(delivered)
        }
      }
    } catch (error) { this.fail(error) }
    finally { link.receiving = false }
  }
  private enqueue(port: NetworkPort, link: Link, frame: Uint8Array): void {
    const routed = this.gateways[port]?.egress(frame)
    if (this.gateways[port] && !routed) return
    frame = routed ?? frame
    if (frame.length < 14 || frame.length > 1514 || link.tx.length >= 32) {
      ++this.state.dropped[port]; return
    }
    link.tx.push(frame.slice())
    if (!link.pumping) void this.pump(port, link)
  }
  private async pump(port: NetworkPort, link: Link): Promise<void> {
    link.pumping = true
    try {
      while (this.links[port] === link && link.tx.length) {
        const frame = link.tx.shift()!
        try { await this.send(port, frame); ++this.state.txFrames[port] }
        catch (error) { ++this.state.dropped[port]; this.fail(error) }
      }
    } finally { link.pumping = false }
  }
  async startForwarding(rules: ForwardRule[]): Promise<void> {
    if (!Array.isArray(rules) || rules.length === 0 || rules.length > 32) throw new Error('포워딩 규칙은 1..32개입니다.')
    const binds = new Set<string>()
    for (const rule of rules) {
      validateForwardRule(rule, this.config)
      if (!this.links[rule.port]) throw new Error(`UART${rule.port} 네트워크를 먼저 켜세요.`)
      const key = `${rule.protocol}:${rule.listenPort}`
      if (binds.has(key)) throw new Error('동일한 로컬 포트 규칙이 중복됩니다.')
      binds.add(key)
    }
    this.stopForwarding()
    try {
      for (const rule of structuredClone(rules)) {
        if (rule.protocol === 'tcp') await this.listenTcp(rule)
        else await this.listenUdp(rule)
      }
      this.state.forwarding = true; this.state.error = ''
      this.log('info', 'network.forwarding_started', { rules: rules.length })
    } catch (error) { this.stopForwarding(); this.fail(error); throw error }
  }
  stopForwarding(port?: NetworkPort): void {
    const stopping = this.handles.filter(handle => port === undefined || handle.port === port)
    this.handles = this.handles.filter(handle => !stopping.includes(handle))
    for (const handle of stopping) handle.close()
    for (const [close, target] of [...this.closers]) if (port === undefined || target === port) close()
    this.state.forwarding = this.handles.length > 0
  }
  private async listenTcp(rule: ForwardRule): Promise<void> {
    const link = this.links[rule.port]!
    const server = net.createServer({ allowHalfOpen: true }, socket => {
      socket.pause()
      if (this.closers.size >= 32) { socket.destroy(); return }
      let connection: TcpConnection | undefined
      let closed = false
      let toSlave = 0, toLocal = 0
      let readPhase = 'connecting', writePhase = 'connecting'
      const started = performance.now()
      const close = (): void => {
        if (closed) return
        closed = true; clearTimeout(timeout); clearInterval(progress); this.closers.delete(close)
        this.state.connections = this.closers.size
        this.log('info', 'tcp.closed', { port: rule.port, toSlave, toLocal, ms: Math.round(performance.now() - started) })
        socket.destroy(); void connection?.close().catch(() => {})
      }
      const timeout = setTimeout(close, 15000)
      const progress = setInterval(() => this.log('info', 'tcp.progress', {
        port: rule.port, toSlave, toLocal, readPhase, writePhase,
        localPending: socket.writableLength, ms: Math.round(performance.now() - started) }), 10000)
      this.log('info', 'tcp.connecting', { port: rule.port, listenPort: rule.listenPort, targetPort: rule.targetPort })
      this.closers.set(close, rule.port); this.state.connections = this.closers.size
      socket.on('error', close); socket.on('close', close); socket.setTimeout(120000, close)
      void link.stack.tcp.connect({ host: rule.targetIp, port: rule.targetPort }).then(async peer => {
        if (closed || this.links[rule.port] !== link) { await peer.close(); return }
        clearTimeout(timeout); connection = peer
        this.log('info', 'tcp.connected', { port: rule.port, ms: Math.round(performance.now() - started) })
        // Keep the native socket in paused/readable mode. Explicit bounded
        // pumps avoid competing Node/Web-stream resume/backpressure machinery
        // that intermittently stalled real NCM echo streams after TCP ACKed data.
        await Promise.all([
          (async () => {
            const writer = peer.writable.getWriter()
            try {
              writePhase = 'local-read'
              for await (const value of socket) {
                const chunk = value as Buffer
                for (let offset = 0; offset < chunk.length; offset += 4096) {
                  if (closed) return
                  const block = chunk.subarray(offset, offset + 4096)
                  writePhase = 'slave-write'
                  await writer.write(block); toSlave += block.length
                }
                writePhase = 'local-read'
              }
              if (!closed) await writer.close()
            } finally { writer.releaseLock() }
          })(),
          (async () => {
            const reader = peer.readable.getReader()
            try {
              while (!closed) {
                readPhase = 'slave-read'
                const item = await reader.read()
                if (item.done) { socket.end(); return }
                if (closed) return
                readPhase = 'local-write'
                await new Promise<void>((resolve, reject) => socket.write(item.value, error => error ? reject(error) : resolve()))
                toLocal += item.value.length
              }
            } finally { reader.releaseLock() }
          })()
        ])
      }).catch(error => { if (!closed) this.fail(error) }).finally(close)
    })
    this.handles.push({ port: rule.port, close: () => { server.close() } })
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject)
      server.listen(rule.listenPort, '127.0.0.1', () => { server.off('error', reject); resolve() })
    })
    server.on('error', error => this.fail(error))
  }
  private async listenUdp(rule: ForwardRule): Promise<void> {
    const link = this.links[rule.port]!
    const socket = dgram.createSocket('udp4')
    const flows = new Map<string, { ready: Promise<void>; queue: Uint8Array[]; last: number; close(): void; wake(): void }>()
    let closed = false
    const sweep = setInterval(() => {
      for (const flow of flows.values()) if (Date.now() - flow.last > 30000) flow.close()
    }, 5000)
    const closeAll = (): void => {
      if (closed) return
      closed = true; clearInterval(sweep)
      for (const flow of [...flows.values()]) flow.close()
      try { socket.close() } catch { /* binding failed */ }
    }
    this.handles.push({ port: rule.port, close: closeAll })
    socket.on('message', (bytes, remote) => {
      if (bytes.length > 1472 || closed) { ++this.state.dropped[rule.port]; return }
      const key = `${remote.address}:${remote.port}`
      let flow = flows.get(key)
      if (!flow) {
        if (this.closers.size >= 32) { ++this.state.dropped[rule.port]; return }
        let peer: UdpSocket | undefined
        let done = false
        const close = (): void => {
          if (done) return
          done = true; flows.delete(key); this.closers.delete(close)
          this.state.connections = this.closers.size
          void peer?.close().catch(() => {})
        }
        flow = { ready: Promise.resolve(), queue: [], last: Date.now(), close, wake: () => {} }
        const current = flow
        flows.set(key, current); this.closers.set(close, rule.port); this.state.connections = this.closers.size
        current.ready = (async () => {
          peer = await link.stack.udp.open()
          if (done) { await peer.close(); return }
          void (async () => {
            for await (const reply of peer!) {
              if (done) break
              if (reply.host === rule.targetIp && reply.port === rule.targetPort && reply.data.length <= 1472) {
                socket.send(reply.data, remote.port, remote.address, error => { if (error) this.fail(error) })
              }
            }
          })().catch(error => { if (!done) this.fail(error) }).finally(close)
        })().catch(error => { this.fail(error); close() })
        // Bounded per-client datagram queue, single writer preserves order.
        const drain = async (): Promise<void> => {
          await current.ready
          if (done || !peer) return
          const writer = peer.writable.getWriter()
          try {
            while (!done && current.queue.length) await writer.write({ host: rule.targetIp,
              port: rule.targetPort, data: current.queue.shift()! })
          } catch (error) { this.fail(error); close() }
          finally { writer.releaseLock() }
        }
        let draining = false
        const schedule = (): void => {
          if (draining) return
          draining = true; void drain().finally(() => { draining = false; if (!done && current.queue.length) schedule() })
        }
        current.wake = schedule
      }
      flow.last = Date.now()
      if (flow.queue.length >= 8) { ++this.state.dropped[rule.port]; return }
      flow.queue.push(Uint8Array.from(bytes)); flow.wake()
    })
    await new Promise<void>((resolve, reject) => {
      socket.once('error', reject)
      socket.bind(rule.listenPort, '127.0.0.1', () => { socket.off('error', reject); resolve() })
    })
    socket.on('error', error => this.fail(error))
  }
  async stop(): Promise<void> {
    this.stopForwarding()
    await this.enable(0, false); await this.enable(1, false)
  }
  private fail(error: unknown): void {
    this.state.error = error instanceof Error ? error.message : String(error)
    this.log('error', 'network.failure', { error: this.state.error })
  }
}
