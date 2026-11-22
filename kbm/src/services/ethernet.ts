export const ETHERNET_MTU = 1514
export const ETHERNET_CHUNK = 64
export function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff
  for (const byte of bytes) {
    crc ^= byte
    for (let i = 0; i < 8; ++i) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0)
  }
  return (~crc) >>> 0
}
export function fragmentFrame(frame: Uint8Array, id: number): Uint8Array[] {
  if (frame.length < 14 || frame.length > ETHERNET_MTU) throw new Error('Invalid Ethernet frame length')
  const crc = crc32(frame)
  const result: Uint8Array[] = []
  for (let offset = 0; offset < frame.length; offset += ETHERNET_CHUNK) {
    const data = frame.slice(offset, offset + ETHERNET_CHUNK)
    const chunk = new Uint8Array(10 + data.length)
    const view = new DataView(chunk.buffer)
    view.setUint16(0, id, true); view.setUint16(2, frame.length, true)
    view.setUint16(4, offset, true); view.setUint32(6, crc, true)
    chunk.set(data, 10); result.push(chunk)
  }
  return result
}
export class EthernetAssembler {
  private id = -1
  private crc = 0
  private offset = 0
  private frame = new Uint8Array()
  accept(chunk: Uint8Array): Uint8Array | undefined {
    if (chunk.length <= 10 || chunk.length > 10 + ETHERNET_CHUNK) return
    const view = new DataView(chunk.buffer, chunk.byteOffset, chunk.byteLength)
    const id = view.getUint16(0, true), total = view.getUint16(2, true), off = view.getUint16(4, true)
    const crc = view.getUint32(6, true)
    if (total < 14 || total > ETHERNET_MTU || off + chunk.length - 10 > total) return
    if (off === 0) { this.id = id; this.crc = crc; this.offset = 0; this.frame = new Uint8Array(total) }
    if (id !== this.id || crc !== this.crc || off !== this.offset || total !== this.frame.length) return
    this.frame.set(chunk.subarray(10), off); this.offset += chunk.length - 10
    if (this.offset === total && crc32(this.frame) === this.crc) {
      this.id = -1
      return this.frame
    }
  }
}
