import type { NetworkConfig } from '../src/shared/network'

function ipBytes(ip: string): Uint8Array { return Uint8Array.from(ip.split('.').map(Number)) }
export function internetChecksum(data: Uint8Array): number {
  let sum = 0
  for (let i = 0; i < data.length; i += 2) sum += (data[i]! << 8) | (data[i + 1] ?? 0)
  while (sum > 0xffff) sum = (sum & 0xffff) + (sum >>> 16)
  return ~sum & 0xffff
}
interface Lease { ip: string; expires: number; offered: boolean }
export class DhcpServer {
  readonly leases = new Map<string, Lease>()
  constructor(private config: NetworkConfig, private mac: Uint8Array, private preferred: number, private dnsServer?: string) {}
  handle(frame: Uint8Array, now = Date.now()): Uint8Array | undefined {
    if (frame.length < 14 + 20 + 8 + 240 || frame[12] !== 8 || frame[13] !== 0) return
    const ihl = (frame[14]! & 15) * 4
    const ip = frame.subarray(14)
    const view = new DataView(frame.buffer, frame.byteOffset, frame.length)
    const total = view.getUint16(16)
    if (frame[14]! >>> 4 !== 4 || ihl < 20 || total < ihl + 8 + 240 || total + 14 > frame.length ||
        frame[23] !== 17 || (view.getUint16(20) & 0x3fff) || internetChecksum(ip.subarray(0, ihl)) !== 0) return
    const udpAt = 14 + ihl
    if (view.getUint16(udpAt) !== 68 || view.getUint16(udpAt + 2) !== 67) return
    const udpLength = view.getUint16(udpAt + 4)
    if (udpLength < 248 || udpLength > total - ihl) return
    if (view.getUint16(udpAt + 6) !== 0) {
      const checksumData = new Uint8Array(12 + udpLength)
      checksumData.set(frame.subarray(26, 34)); checksumData[9] = 17
      new DataView(checksumData.buffer).setUint16(10, udpLength)
      checksumData.set(frame.subarray(udpAt, udpAt + udpLength), 12)
      if (internetChecksum(checksumData) !== 0) return
    }
    const bootp = frame.subarray(udpAt + 8, udpAt + udpLength)
    if (bootp[0] !== 1 || bootp[1] !== 1 || bootp[2] !== 6 || bootp[3] !== 0 ||
        bootp[236] !== 99 || bootp[237] !== 130 || bootp[238] !== 83 || bootp[239] !== 99 ||
        bootp.subarray(24, 28).some(byte => byte !== 0)) return // no relay agents
    const opts = new Map<number, Uint8Array>()
    for (let at = 240; at < bootp.length;) {
      const code = bootp[at++]!
      if (code === 255) break
      if (code === 0) continue
      const size = bootp[at++]
      if (size === undefined || at + size > bootp.length) return
      opts.set(code, bootp.slice(at, at + size)); at += size
    }
    const kind = opts.get(53)?.[0]
    const key = Array.from(bootp.subarray(28, 34), n => n.toString(16).padStart(2, '0')).join(':')
    for (const [mac, lease] of this.leases) if (lease.expires <= now) this.leases.delete(mac)
    if (kind === 7 || kind === 4) { this.leases.delete(key); return }
    if (kind !== 1 && kind !== 3 && kind !== 8) return
    const server = ipBytes(this.config.serverIp)
    const serverId = opts.get(54)
    if (serverId && (serverId.length !== 4 || serverId.some((n, i) => n !== server[i]))) return
    const prefix = this.config.serverIp.slice(0, this.config.serverIp.lastIndexOf('.') + 1)
    let lease = this.leases.get(key)
    if (!lease && kind !== 8) {
      const used = new Set(Array.from(this.leases.values(), l => l.ip))
      const candidates = [this.preferred, ...Array.from({ length: this.config.poolEnd - this.config.poolStart + 1 }, (_, i) => this.config.poolStart + i)]
      const last = candidates.find(n => n <= this.config.poolEnd && !used.has(prefix + n))
      if (last === undefined) return
      lease = { ip: prefix + last, expires: now + 30000, offered: true }
      this.leases.set(key, lease)
    }
    const requested = opts.get(50) ?? bootp.subarray(12, 16)
    const mismatch = kind === 3 && (!lease || requested.length !== 4 || requested.some((n, i) => n !== ipBytes(lease!.ip)[i]))
    const replyKind = mismatch ? 6 : kind === 1 ? 2 : 5
    if (lease && kind === 3 && !mismatch) {
      lease.expires = now + this.config.leaseSeconds * 1000; lease.offered = false
    }
    const options = [53, 1, replyKind, 54, 4, ...server, 1, 4, 255, 255, 255, 0,
      3, 4, ...server] // Router is the app-backed master, x.x.x.1.
    if (kind !== 8 && !mismatch) {
      const seconds = this.config.leaseSeconds
      options.push(51, 4, seconds >>> 24, seconds >>> 16 & 255, seconds >>> 8 & 255, seconds & 255)
    }
    if (this.dnsServer) options.push(6, 4, ...ipBytes(this.dnsServer))
    // Emulation omits DNS; host sharing advertises its gateway DNS relay.
    // Low USB link speed keeps the
    // ordinary physical NIC preferred by the host's automatic route metric.
    options.push(255)
    const payload = new Uint8Array(Math.max(300, 240 + options.length))
    payload.set(bootp.subarray(0, 44)); payload[0] = 2; payload[3] = 0
    payload.fill(0, 12, 28)
    if (kind !== 8 && !mismatch && lease) payload.set(ipBytes(lease.ip), 16)
    payload.set(server, 20); payload.set([99, 130, 83, 99], 236); payload.set(options, 240)
    const out = new Uint8Array(14 + 20 + 8 + payload.length)
    const ov = new DataView(out.buffer)
    out.fill(255, 0, 6); out.set(this.mac, 6); ov.setUint16(12, 0x0800)
    out[14] = 0x45; ov.setUint16(16, out.length - 14); out[22] = 64; out[23] = 17
    out.set(server, 26); out.fill(255, 30, 34)
    ov.setUint16(24, internetChecksum(out.subarray(14, 34)))
    ov.setUint16(34, 67); ov.setUint16(36, 68); ov.setUint16(38, payload.length + 8)
    out.set(payload, 42)
    return out
  }
}
