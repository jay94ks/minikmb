import { randomBytes } from 'node:crypto'
import { validIp, type PortScanRequest, type PortScanState } from '../src/shared/network'
import { ipBytes, ipText, packet, tcpFrame, validTransport } from './ipv4-packets'
export const emptyScan = (): PortScanState => ({ running: false, target: '', start: 1, end: 1024, completed: 0, total: 0, open: [], closed: 0, filtered: 0, error: '' })
export class PortScanner {
  private status = emptyScan()
  private generation = 0
  private sequence = randomBytes(4).readUInt32LE()
  private mac: Uint8Array | undefined
  private arpDone: (() => void) | undefined
  private arpCancel: (() => void) | undefined
  private probes = new Map<number, { sequence: number; source: number; finish(result: 'open' | 'closed' | 'filtered'): void }>()
  constructor(private serverIp: string, private serverMac: Uint8Array, private send: (frame: Uint8Array) => void) {}
  state(): PortScanState { return structuredClone(this.status) }
  stop(): void { ++this.generation; this.status.running = false; this.arpCancel?.(); this.arpDone = this.arpCancel = undefined; for (const p of this.probes.values()) p.finish('filtered'); this.probes.clear() }
  start(request: PortScanRequest): void {
    const prefix = this.serverIp.slice(0, this.serverIp.lastIndexOf('.') + 1)
    if (!request || !validIp(request.target) || !request.target.startsWith(prefix) || request.target === this.serverIp || ['0', '255'].includes(request.target.split('.')[3]!) || !Number.isInteger(request.start) || !Number.isInteger(request.end) || request.start < 1 || request.end > 65535 || request.end < request.start || !Number.isInteger(request.timeoutMs) || request.timeoutMs < 250 || request.timeoutMs > 5000) throw new Error('현재 NCM 타깃 IP와 포트 범위(1..65535), 제한 시간(250..5000ms)을 확인하세요.')
    this.stop(); const generation = this.generation; this.mac = undefined
    this.status = { ...emptyScan(), target: request.target, start: request.start, end: request.end, running: true, total: request.end - request.start + 1 }
    void this.run(request, generation)
  }
  private async run(request: PortScanRequest, generation: number): Promise<void> {
    try {
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => { this.arpDone = this.arpCancel = undefined; reject(new Error('타깃의 네트워크 응답이 없습니다. IP 주소를 확인하세요.')) }, 2000)
        this.arpDone = () => { clearTimeout(timer); this.arpDone = this.arpCancel = undefined; resolve() }
        this.arpCancel = () => { clearTimeout(timer); reject(new Error('스캔 중지')) }
        const f = new Uint8Array(42), v = new DataView(f.buffer)
        f.fill(255, 0, 6); f.set(this.serverMac, 6); v.setUint16(12, 0x806); v.setUint16(14, 1); v.setUint16(16, 0x800); f[18] = 6; f[19] = 4; v.setUint16(20, 1)
        f.set(this.serverMac, 22); f.set(ipBytes(this.serverIp), 28); f.set(ipBytes(request.target), 38); this.send(f)
      })
      let next = request.start
      await Promise.all(Array.from({ length: Math.min(8, this.status.total) }, (_, slot) => (async () => {
        while (generation === this.generation && next <= request.end) {
          const port = next++, sequence = this.sequence++ >>> 0, source = 41000 + slot
          const result = await new Promise<'open' | 'closed' | 'filtered'>(resolve => {
            const timer = setTimeout(() => finish('filtered'), request.timeoutMs)
            const finish = (result: 'open' | 'closed' | 'filtered') => { clearTimeout(timer); this.probes.delete(port); resolve(result) }
            this.probes.set(port, { sequence, source, finish }); this.send(tcpFrame(this.serverMac, this.mac!, this.serverIp, request.target, source, port, sequence, 0, 2))
          })
          if (generation !== this.generation) return
          this.status.completed++
          if (result === 'open') { this.status.open.push(port); this.status.open.sort((a, b) => a - b) } else this.status[result]++
        }
      })()))
    } catch (e) { if (generation === this.generation) this.status.error = String(e) }
    finally { if (generation === this.generation) this.status.running = false }
  }
  receive(frame: Uint8Array): boolean {
    if (!this.status.running) return false
    if (frame.length >= 42 && frame[12] === 8 && frame[13] === 6 && frame[18] === 6 && frame[19] === 4 && ipText(frame.subarray(28, 32)) === this.status.target && frame.subarray(6, 12).every((x, i) => x === frame[22 + i])) { this.mac = frame.slice(22, 28); this.arpDone?.(); return false }
    const p = packet(frame)
    if (!p || p.protocol !== 6 || p.source !== this.status.target || p.destination !== this.serverIp || !validTransport(p)) return false
    const probe = this.probes.get(p.sourcePort)
    if (!probe || probe.source !== p.destinationPort) return false
    const v = new DataView(p.frame.buffer, p.frame.byteOffset, p.frame.byteLength), flags = frame[p.offset + 13]!
    if (!(flags & 0x10) || v.getUint32(p.offset + 8) !== (probe.sequence + 1 >>> 0)) return false
    if ((flags & 0x12) === 0x12 && !(flags & 4)) { this.send(tcpFrame(this.serverMac, p.frame.subarray(6, 12), this.serverIp, p.source, probe.source, p.sourcePort, v.getUint32(p.offset + 8), (v.getUint32(p.offset + 4) + 1) >>> 0, 0x14)); probe.finish('open'); return true }
    if (flags & 4) { probe.finish('closed'); return true }
    return false
  }
}
