import type { Packet } from '../services/protocol'
export const SLAVE_BOOT_EVENT = 0xf9
export function slaveBoot(packet: Packet): { port: 0 | 1; firmware: number; resetReason: number; usbState: number; queued: number } | undefined {
  const p = packet.payload
  if (packet.op !== SLAVE_BOOT_EVENT || p.length !== 17 || (p[0] !== 0 && p[0] !== 1) || p[1] !== 1 || p[2] !== 3 || p[16] !== 0 || !p.subarray(5,13).some(value => value !== 0)) return
  return { port: p[0], firmware: p[3]! | p[4]! << 8, resetReason: p[13]!, usbState: p[14]!, queued: p[15]! }
}
