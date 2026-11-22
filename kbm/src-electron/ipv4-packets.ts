import { internetChecksum } from './dhcp'
export interface IpPacket { frame: Uint8Array; offset: number; protocol: 6 | 17; source: string; destination: string; sourcePort: number; destinationPort: number }
export const ipBytes = (ip: string) => Uint8Array.from(ip.split('.').map(Number))
export const ipText = (bytes: Uint8Array) => Array.from(bytes).join('.')
export function packet(frame: Uint8Array): IpPacket | undefined {
  if (frame.length < 42 || frame[12] !== 8 || frame[13] !== 0 || frame[14]! >>> 4 !== 4) return
  const v = new DataView(frame.buffer, frame.byteOffset, frame.byteLength), header = (frame[14]! & 15) * 4, length = v.getUint16(16), protocol = frame[23]
  if (header < 20 || length < header + (protocol === 6 ? 20 : 8) || length + 14 > frame.length || v.getUint16(20) & 0x3fff || internetChecksum(frame.subarray(14, 14 + header)) !== 0 || (protocol !== 6 && protocol !== 17)) return
  const offset = 14 + header, transport = length - header
  if (protocol === 6 && ((frame[offset + 12]! >>> 4) < 5 || (frame[offset + 12]! >>> 4) * 4 > transport)) return
  if (protocol === 17 && v.getUint16(offset + 4) !== transport) return
  return { frame: frame.subarray(0, 14 + length), offset, protocol, source: ipText(frame.subarray(26, 30)), destination: ipText(frame.subarray(30, 34)), sourcePort: v.getUint16(offset), destinationPort: v.getUint16(offset + 2) }
}
function transportChecksum(frame: Uint8Array, offset: number): number {
  const bytes = new Uint8Array(12 + frame.length - offset), v = new DataView(bytes.buffer)
  bytes.set(frame.subarray(26, 34)); bytes[9] = frame[23]!; v.setUint16(10, frame.length - offset); bytes.set(frame.subarray(offset), 12)
  return internetChecksum(bytes)
}
export function validTransport(p: IpPacket): boolean { return p.protocol === 17 && !p.frame[p.offset + 6] && !p.frame[p.offset + 7] || transportChecksum(p.frame, p.offset) === 0 }
export function checksums(frame: Uint8Array, offset: number): void {
  const v = new DataView(frame.buffer, frame.byteOffset, frame.byteLength), at = offset + (frame[23] === 6 ? 16 : 6)
  v.setUint16(24, 0); v.setUint16(24, internetChecksum(frame.subarray(14, offset)))
  v.setUint16(at, 0); v.setUint16(at, transportChecksum(frame, offset) || 0xffff)
}
export function rewrite(p: IpPacket, source: string, sourcePort: number, destination: string, destinationPort: number): Uint8Array {
  const frame = p.frame.slice(), v = new DataView(frame.buffer)
  frame.set(ipBytes(source), 26); frame.set(ipBytes(destination), 30); v.setUint16(p.offset, sourcePort); v.setUint16(p.offset + 2, destinationPort); checksums(frame, p.offset); return frame
}
export function tcpFrame(sourceMac: Uint8Array, destMac: Uint8Array, sourceIp: string, destIp: string, sourcePort: number, destPort: number, sequence: number, ack: number, flags: number): Uint8Array {
  const frame = new Uint8Array(54), v = new DataView(frame.buffer)
  frame.set(destMac); frame.set(sourceMac, 6); v.setUint16(12, 0x800); frame[14] = 0x45; v.setUint16(16, 40); frame[22] = 64; frame[23] = 6
  frame.set(ipBytes(sourceIp), 26); frame.set(ipBytes(destIp), 30); v.setUint16(34, sourcePort); v.setUint16(36, destPort); v.setUint32(38, sequence); v.setUint32(42, ack); frame[46] = 0x50; frame[47] = flags; v.setUint16(48, 65535); checksums(frame, 34); return frame
}
