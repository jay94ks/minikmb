import { crc32 } from 'node:zlib'
import type { MiniConnection } from './minikmb-connection'
import type { MiniFrame } from '../src/services/minikmb-protocol'

export class MiniNetworkWire {
  private enabled = false
  private txId = 0
  private rxId = 0
  private sequence = 0
  private consumed = 0n
  private credit = 32768n
  private updating = false
  private partial: { id: number; length: number; crc: number; data: Uint8Array; used: number; deadline: number } | undefined
  private unsubscribe: () => void
  dropped = 0
  constructor(private client: MiniConnection, private deliver: (frame: Uint8Array) => void) { this.unsubscribe = client.subscribe(frame => this.receive(frame)) }
  private async updateCredit(): Promise<void> {
    if (this.updating) return
    this.updating = true
    try {
      do {
        this.credit += this.consumed; this.consumed = 0n
        const b = new Uint8Array(12), v = new DataView(b.buffer); b[0] = 4; v.setBigUint64(4, this.credit, true)
        await this.client.command(0, 6, b)
      } while (this.consumed >= 16384n)
    } finally { this.updating = false }
  }
  async enable(on: boolean): Promise<void> {
    this.partial = undefined
    if (on) { await this.updateCredit(); this.enabled = true }
    else this.enabled = false
    try { await this.client.command(4, 0x401, Uint8Array.of(Number(on))) }
    catch (e) { this.enabled = false; throw e }
  }
  async send(frame: Uint8Array): Promise<void> {
    if (!this.enabled || frame.length < 14 || frame.length > 1518) throw new Error('NCM 링크가 준비되지 않았습니다.')
    if (this.txId === 0xffffffff) throw new Error('NCM 프레임 번호 소진 · 다시 연결하세요.')
    const id = ++this.txId, crc = crc32(frame)
    for (let offset = 0; offset < frame.length; offset += 1008) {
      if (!this.enabled) throw new Error('NCM 전송 취소')
      const data = frame.subarray(offset, offset + 1008), b = new Uint8Array(16 + data.length), v = new DataView(b.buffer)
      v.setUint32(0, id, true); v.setUint16(4, offset, true); v.setUint16(6, frame.length, true); v.setUint32(8, crc, true); b.set(data, 16)
      // Wait for each real fragment ACK. HID/SYSTEM have independent queues.
      await this.client.command(4, 0x402, b)
    }
  }
  private receive(frame: MiniFrame): void {
    if (frame.kind !== 4 || frame.channel !== 4 || frame.opcode !== 0x480) return
    this.consumed += BigInt(frame.payload.length)
    if (this.consumed >= 16384n) void this.updateCredit().catch(() => { this.enabled = false })
    if (!this.enabled) return
    const b = frame.payload, v = new DataView(b.buffer, b.byteOffset, b.byteLength)
    if (this.sequence && frame.sequence !== this.sequence + 1) { this.partial = undefined; this.dropped++ }
    this.sequence = frame.sequence
    if (b.length <= 16) { this.partial = undefined; this.dropped++; return }
    const id = v.getUint32(0, true), offset = v.getUint16(4, true), length = v.getUint16(6, true), crc = v.getUint32(8, true)
    if (length < 14 || length > 1518 || offset >= length || b.length - 16 !== Math.min(1008, length - offset) || v.getUint32(12, true)) { this.partial = undefined; this.dropped++; return }
    if (offset === 0 && id > this.rxId) {
      if (this.partial) this.dropped++
      this.rxId = id; this.partial = { id, length, crc, used: 0, data: new Uint8Array(length), deadline: performance.now() + 1000 }
    }
    const p = this.partial
    if (!p || p.id !== id || p.length !== length || p.crc !== crc || offset !== p.used || performance.now() > p.deadline) { this.partial = undefined; this.dropped++; return }
    p.data.set(b.subarray(16), offset); p.used += b.length - 16
    if (p.used === length) { this.partial = undefined; if (crc32(p.data) === crc) this.deliver(p.data); else this.dropped++ }
  }
  dispose(): void { this.enabled = false; this.partial = undefined; this.unsubscribe() }
}
