import type { NetworkConfig, NetworkState, ForwardRule, PortScanRequest, PortScanState } from './network'
export type MiniNetworkMode = 'off' | 'emulation' | 'host'
import type { SpecialInputState, KeyChordRequest } from './special-input'
export interface MiniDiskImage { id: number; kind: 'optical' | 'disk'; name: string; size: number; format: number; mounted: boolean; path: number; writable: boolean }
export interface MiniTransferProgress { bytesPerSecond: number | null; remainingSeconds: number | null; elapsedSeconds: number }
export interface MiniStorageState {
  busy: boolean; action: string; done: number; total: number; error: string
  image: { id: number; size: number; mounted: boolean; name: string; crc: number } | null
  disk: { id: number; sectors: number; lease: number; format: number; state: number; writable: boolean } | null
  card: { present: boolean; initialized: boolean; uuid: string; userLength: number; revision: number } | null
  sectorHex: string; userText: string
  capabilities: number; opticalEnabled: boolean; opticalApplied: boolean; images: MiniDiskImage[]
  transfer: MiniTransferProgress | null
}
export type MiniStorageAction =
  | { kind: 'channel-name-set'; name: string }
  | { kind: 'refresh' | 'optical-upload' | 'optical-mount' | 'optical-unmount' | 'folder-share' | 'disk-close' | 'disk-sync' | 'card-init' | 'card-user-read' | 'cancel' }
  | { kind: 'disk-upload'; format: 1 | 2 | 3 }
  | { kind: 'optical-set'; enabled: boolean }
  | { kind: 'disk-create'; format: 1 | 2 | 3; sizeMiB: number }
  | { kind: 'disk-format'; id: number; format: 1 | 2 | 3; confirmed: boolean }
  | { kind: 'disk-open'; id: number; path: 0 | 1 | 2; writable: boolean }
  | { kind: 'disk-read'; lba: number }
  | { kind: 'disk-write'; lba: number; hex: string; confirmed: boolean }
  | { kind: 'card-user-write'; text: string }
  | { kind: 'card-format'; format: 1 | 2; confirmed: boolean }
export interface MiniToolsState {
  uid: string; features: number; input: SpecialInputState
  serial: { open: boolean; baud: number; dataBits: number; parity: number; stopBits: number; txQueued: number; rxQueued: number; txDiscarded: number; rxDiscarded: number; received: number[]; bytesReceived: number }
  network: NetworkState; config: NetworkConfig; rules: ForwardRule[]; networkUp: boolean; networkDropped: number
  networkMode: MiniNetworkMode; scan: PortScanState
  diagnostics: { uptime: number; crcErrors: number; framingErrors: number; eventDropped: number; logDropped: number; storageState: number }
  storage: MiniStorageState
}
export type { NetworkConfig, ForwardRule, KeyChordRequest }
export interface MiniToolsApi {
  rename(uid: string, name: string): Promise<void>
  state(uid: string): Promise<MiniToolsState>
  terminal(uid: string): Promise<void>
  serialWrite(uid: string, bytes: number[]): Promise<void>
  chord(uid: string, request: KeyChordRequest): Promise<void>
  system(uid: string, usage: number, confirmed: boolean): Promise<void>
  release(uid: string): Promise<void>
  deactivate(uid: string): Promise<void>
  configure(uid: string, config: NetworkConfig): Promise<void>
  network(uid: string, mode: MiniNetworkMode): Promise<void>
  scan(uid: string, request: PortScanRequest | null): Promise<void>
  forward(uid: string, rules: ForwardRule[]): Promise<void>
  stopForward(uid: string): Promise<void>
  storage(uid: string, action: MiniStorageAction): Promise<void>
}
