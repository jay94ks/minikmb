import net from 'node:net'
import dgram from 'node:dgram'
import dns from 'node:dns'
import type { NetworkStack, TcpConnection, UdpSocket } from 'tcpip'
import { packet, rewrite, validTransport, type IpPacket } from './ipv4-packets'

export interface HostGatewayIo { tcp(host: string, port: number): net.Socket; udp(host: string, port: number): dgram.Socket; dns(): string }
const io: HostGatewayIo = {
  tcp: (host, port) => net.createConnection({ host, port, allowHalfOpen: true }),
  udp: (host, port) => { const s = dgram.createSocket(net.isIP(host) === 6 ? 'udp6' : 'udp4'); s.connect(port, host); return s },
  dns: () => { const server = dns.getServers().find(s => net.isIP(s)); if (!server) throw new Error('관리 PC의 DNS 서버를 찾을 수 없습니다.'); return server }
}
interface Flow { key: string; protocol: 6 | 17; client: string; clientPort: number; remote: string; remotePort: number; dial: string; localPort: number; last: number; close(): void; peer?: TcpConnection | UdpSocket; socket?: net.Socket | dgram.Socket; closed: boolean }
const TCP_BASE = 20000, TCP_SLOTS = 8, UDP_BASE = 20032, UDP_SLOTS = 24

// User-space IPv4 TCP/UDP gateway. No host NIC, route, firewall or ICS changes.
// tcpip 0.4 listeners have no close API: reuse a fixed pool per cached stack.
export class HostGateway {
  private enabled = false
  private server = ''
  private ready: Promise<void> | undefined
  private flows = new Map<string, Flow>()
  private tcp: (Flow | undefined)[] = Array(TCP_SLOTS)
  private udp: (Flow | undefined)[] = Array(UDP_SLOTS)
  private sweep: ReturnType<typeof setInterval> | undefined
  constructor(private stack: NetworkStack, private room: () => boolean, private dropped: () => void, private host: HostGatewayIo = io, private report: (error: unknown) => void = () => {}) {}
  get count(): number { return this.flows.size }
  get active(): boolean { return this.enabled }
  async start(server: string): Promise<void> {
    this.stop(); this.server = server
    if (!this.ready) this.ready = (async () => {
      for (let slot = 0; slot < TCP_SLOTS; slot++) {
        const listener = await this.stack.tcp.listen({ host: '0.0.0.0', port: TCP_BASE + slot })
        void (async () => { for await (const peer of listener) { const flow = this.tcp[slot]; if (!this.enabled || !flow || flow.peer) { void peer.close().catch(() => {}); continue }; void this.bridgeTcp(flow, peer) } })().catch(() => this.stop())
      }
    })()
    await this.ready
    this.enabled = true
    this.sweep = setInterval(() => { for (const flow of this.flows.values()) if (Date.now() - flow.last > (flow.protocol === 6 ? 120000 : 30000)) flow.close() }, 5000)
  }
  stop(): void { this.enabled = false; if (this.sweep) clearInterval(this.sweep); this.sweep = undefined; for (const f of [...this.flows.values()]) f.close() }
  private newFlow(p: IpPacket): Flow | undefined {
    if (this.flows.size >= 32 || !this.room()) return
    const slots = p.protocol === 6 ? this.tcp : this.udp, slot = slots.findIndex(x => !x)
    if (slot < 0) return
    const key = `${p.protocol}:${p.source}:${p.sourcePort}:${p.destination}:${p.destinationPort}`
    const flow: Flow = { key, protocol: p.protocol, client: p.source, clientPort: p.sourcePort, remote: p.destination, remotePort: p.destinationPort,
      dial: p.destination === this.server && p.destinationPort === 53 ? this.host.dns() : p.destination,
      localPort: (p.protocol === 6 ? TCP_BASE : UDP_BASE) + slot, last: Date.now(), closed: false,
      close: () => { if (flow.closed) return; flow.closed = true; this.flows.delete(key); if (slots[slot] === flow) slots[slot] = undefined; if (flow.protocol === 6) (flow.socket as net.Socket | undefined)?.destroy(); else { try { (flow.socket as dgram.Socket | undefined)?.close() } catch { /* not bound */ } }; void flow.peer?.close().catch(() => {}) } }
    slots[slot] = flow; this.flows.set(key, flow); return flow
  }
  async ingress(frame: Uint8Array): Promise<Uint8Array | undefined> {
    const p = packet(frame)
    if (!p) return frame
    if (p.destination === this.server && (p.destinationPort >= TCP_BASE && p.destinationPort < TCP_BASE + TCP_SLOTS || p.destinationPort >= UDP_BASE && p.destinationPort < UDP_BASE + UDP_SLOTS)) return
    if (!this.enabled) return frame
    const prefix = this.server.slice(0, this.server.lastIndexOf('.') + 1), dns = p.destination === this.server && p.destinationPort === 53
    if (p.destination.startsWith(prefix) && !dns) return frame
    const ip = p.destination.split('.').map(Number)
    if (!p.source.startsWith(prefix) || p.source === this.server || ['0', '255'].includes(p.source.split('.')[3]!) || !p.sourcePort || !p.destinationPort || p.frame[22]! <= 1 || !validTransport(p) || (!dns && (ip[0] === 0 || ip[0] === 127 || ip[0]! >= 224 || ip[0] === 169 && ip[1] === 254))) { this.dropped(); return }
    const key = `${p.protocol}:${p.source}:${p.sourcePort}:${p.destination}:${p.destinationPort}`
    let flow = this.flows.get(key)
    if (!flow) {
      if (p.protocol === 6 && (p.frame[p.offset + 13]! & 0x17) !== 2) return
      try { flow = this.newFlow(p); if (flow?.protocol === 17) await this.openUdp(flow) }
      catch (e) { flow?.close(); this.report(e); this.dropped(); return }
      if (!flow) { this.dropped(); return }
    }
    if (flow.closed || !this.enabled) return
    flow.last = Date.now(); return rewrite(p, p.source, p.sourcePort, this.server, flow.localPort)
  }
  egress(frame: Uint8Array): Uint8Array | undefined {
    const p = packet(frame)
    if (!p || p.source !== this.server) return frame
    const index = p.sourcePort - (p.protocol === 6 ? TCP_BASE : UDP_BASE), slots = p.protocol === 6 ? this.tcp : this.udp
    if (index < 0 || index >= slots.length) return frame
    const f = slots[index]
    if (!this.enabled || !f || f.closed || f.client !== p.destination || f.clientPort !== p.destinationPort) return
    return rewrite(p, f.remote, f.remotePort, f.client, f.clientPort)
  }
  private async bridgeTcp(flow: Flow, peer: TcpConnection): Promise<void> {
    flow.peer = peer
    let peerEnded = false
    try {
      const socket = this.host.tcp(flow.dial, flow.remotePort); flow.socket = socket; socket.pause()
      socket.on('error', e => { this.report(e); flow.close() }); socket.on('close', flow.close); socket.setTimeout(120000, flow.close)
      await new Promise<void>((resolve, reject) => { const timer = setTimeout(() => reject(new Error('connect timeout')), 10000); socket.once('connect', () => { clearTimeout(timer); resolve() }); socket.once('error', e => { clearTimeout(timer); reject(e) }); socket.once('close', () => { clearTimeout(timer); reject(new Error('closed')) }) })
      if (flow.closed || !this.enabled) return
      await Promise.all([
        (async () => { const writer = peer.writable.getWriter(); try { for await (const data of socket) { if (flow.closed) return; flow.last = Date.now(); const b = data as Buffer; for (let at = 0; at < b.length; at += 4096) { if (flow.closed) return; await writer.write(b.subarray(at, at + 4096)) } }; if (!flow.closed) await writer.close() } finally { writer.releaseLock() } })(),
        (async () => { const reader = peer.readable.getReader(); try { while (!flow.closed) { const data = await reader.read(); if (data.done) { peerEnded = true; socket.end(); return }; if (flow.closed) return; flow.last = Date.now(); await new Promise<void>((resolve, reject) => socket.write(data.value, e => e ? reject(e) : resolve())) } } finally { reader.releaseLock() } })()
      ])
    } catch (e) { if (!flow.closed && !peerEnded) { this.report(e); this.dropped() } }
    finally { flow.close() }
  }
  private async openUdp(flow: Flow): Promise<void> {
    const peer = await this.stack.udp.open({ host: this.server, port: flow.localPort }); flow.peer = peer
    if (flow.closed || !this.enabled) { await peer.close(); return }
    const socket = this.host.udp(flow.dial, flow.remotePort); flow.socket = socket
    await new Promise<void>((resolve, reject) => { const timer = setTimeout(() => reject(new Error('UDP connect timeout')), 2000); socket.once('connect', () => { clearTimeout(timer); resolve() }); socket.once('error', e => { clearTimeout(timer); reject(e) }) })
    if (flow.closed || !this.enabled) { flow.close(); return }
    const queue: Uint8Array[] = []; let pumping = false
    const pump = async (): Promise<void> => {
      if (pumping || flow.closed) return; pumping = true
      const writer = peer.writable.getWriter()
      try { while (!flow.closed && queue.length) await writer.write({ host: flow.client, port: flow.clientPort, data: queue.shift()! }) }
      catch { flow.close() } finally { writer.releaseLock(); pumping = false }
    }
    socket.on('message', bytes => { if (flow.closed) return; if (bytes.length > 1472 || queue.length >= 8) { this.dropped(); return }; flow.last = Date.now(); queue.push(Uint8Array.from(bytes)); void pump() })
    socket.on('error', e => { this.report(e); flow.close() })
    void (async () => {
      try { for await (const data of peer) { if (flow.closed) return; if (data.host !== flow.client || data.port !== flow.clientPort || data.data.length > 1472) continue; flow.last = Date.now(); await new Promise<void>((resolve, reject) => socket.send(data.data, e => e ? reject(e) : resolve())) } }
      catch { /* cancellation or transport error */ } finally { flow.close() }
    })()
  }
}
