import { crc32 } from './ethernet'
export interface MiniFrame { kind: number; channel: number; opcode: number; session: bigint; sequence: number; payload: Uint8Array }
export function miniEncode(frame: MiniFrame): Uint8Array {
  if (frame.payload.length > 1024) throw new Error('payload too large')
  const raw = new Uint8Array(28 + frame.payload.length), v = new DataView(raw.buffer)
  v.setUint16(0, 0x4d4b, true); raw[2] = 1; raw[3] = frame.kind; raw[5] = frame.channel
  v.setUint16(6, frame.opcode, true); v.setBigUint64(8, frame.session, true)
  v.setUint32(16, frame.sequence, true); v.setUint16(20, frame.payload.length, true)
  raw.set(frame.payload, 24); v.setUint32(raw.length - 4, crc32(raw.subarray(0, -4)), true)
  const out = [0]; let codeAt = 0, code = 1
  for (const b of raw) {
    if (b) { out.push(b); if (++code < 255) continue }
    out[codeAt] = code; codeAt = out.length; out.push(0); code = 1
  }
  out[codeAt] = code; out.push(0)
  // End a full-packet CDC write with a short packet; empty COBS frames are ignored.
  if (out.length % 64 === 0) out.push(0)
  return Uint8Array.from(out)
}
export function miniDecode(encoded: Uint8Array): MiniFrame {
  const raw: number[] = []
  for (let i = 0; i < encoded.length;) {
    const code = encoded[i++]!
    if (!code || i + code - 1 > encoded.length) throw new Error('invalid COBS')
    for (let n = 1; n < code; n++) raw.push(encoded[i++]!)
    if (code < 255 && i < encoded.length) raw.push(0)
  }
  const b = Uint8Array.from(raw), v = new DataView(b.buffer)
  if (b.length < 28 || v.getUint16(0, true) !== 0x4d4b || b[2] !== 1 || b[4] || v.getUint16(22, true) ||
      b.length !== 28 + v.getUint16(20, true) || v.getUint16(20, true) > 1024 ||
      crc32(b.subarray(0, -4)) !== v.getUint32(b.length - 4, true)) throw new Error('invalid management frame')
  return { kind: b[3]!, channel: b[5]!, opcode: v.getUint16(6, true), session: v.getBigUint64(8, true),
    sequence: v.getUint32(16, true), payload: b.slice(24, -4) }
}
export class MiniDecoder {
  private bytes: number[] = []
  private discard = false
  feed(chunk: Uint8Array): MiniFrame[] {
    const frames: MiniFrame[] = []
    for (const b of chunk) {
      if (!b) {
        if (!this.discard && this.bytes.length) { try { frames.push(miniDecode(Uint8Array.from(this.bytes))) } catch { /* CRC/framing rejection */ } }
        this.bytes = []; this.discard = false
      } else if (!this.discard) {
        if (this.bytes.length === 1057) { this.bytes = []; this.discard = true } else this.bytes.push(b)
      }
    }
    return frames
  }
}
