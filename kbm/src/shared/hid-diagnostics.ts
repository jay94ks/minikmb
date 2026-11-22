import type { Packet } from '../services/protocol'
export interface HidSnapshot {
  flags: number; mounted: boolean; ready: boolean; suspended: boolean; hidReady: boolean; stable: boolean
  waiting: boolean; submitted: boolean; queue: number; reportId: number; watchdogReset: boolean
  uptimeMs: number; busResets: number; usbEpoch: number; reportsSubmitted: number; reportsCompleted: number
  waitMs: number; setups: number; queueRejected: number; rxErrors: number; activity: number
}
export interface HidAlarm { port: 0 | 1; code: string; level: 'info' | 'warn' | 'error'; message: string; recovered?: boolean }
export interface HidPortState { online: boolean; seenAt: number; bootCount: number; firmware?: number; snapshot?: HidSnapshot | undefined; alarms: HidAlarm[] }
export type HidDiagnosticsState = [HidPortState, HidPortState]
export function decodeHidDiagnostics(packet: Packet): { port: 0 | 1; snapshot: HidSnapshot } | undefined {
  const p = packet.payload
  if (packet.op !== 0xfa || p.length !== 53 || (p[0] !== 0 && p[0] !== 1)) return
  const d = p.subarray(1)
  if (d[0] !== 1 || d[1]! & 128 || d[2]! > 32 || d[3]! > 3 || d[5] || d[6] || d[7] || d.subarray(48).some(value => value !== 0)) return
  const v = new DataView(d.buffer, d.byteOffset, d.byteLength), flags = d[1]!
  return { port: p[0], snapshot: { flags, mounted: Boolean(flags & 1), ready: Boolean(flags & 2), suspended: Boolean(flags & 4), hidReady: Boolean(flags & 8), stable: Boolean(flags & 16), waiting: Boolean(flags & 32), submitted: Boolean(flags & 64), queue: d[2]!, reportId: d[3]!, watchdogReset: Boolean(d[4]! & 1), uptimeMs: v.getUint32(8,true), busResets: v.getUint32(12,true), usbEpoch: v.getUint32(16,true), reportsSubmitted: v.getUint32(20,true), reportsCompleted: v.getUint32(24,true), waitMs: v.getUint32(28,true), setups: v.getUint32(32,true), queueRejected: v.getUint32(36,true), rxErrors: v.getUint32(40,true), activity: v.getUint32(44,true) } }
}
