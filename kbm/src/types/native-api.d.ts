import type { NetworkConfig, NetworkPort, ForwardRule, NetworkState } from '../shared/network'
import type { MiniInput, MiniInventory, MiniKeyboardState } from '../shared/minikmb'
import type { MiniVideoInfo, MiniVideoFrame, MiniVideoMode } from '../shared/mini-capture'
import type { MiniToolsApi } from '../shared/mini-tools'
import type { DiskSnapshotInfo, DiskReadRequest, DiskTransferMetrics } from '../shared/disk'
import type { LogState, LogFields } from '../shared/logging'
import type { WolAdapter, WolConfig, WakeResult } from '../shared/wake'
import type { KeyChordRequest, SpecialInputState } from '../shared/special-input'
import type { SystemControlResult } from '../shared/system-controls'
import type { HidAlarm, HidDiagnosticsState } from '../shared/hid-diagnostics'
export interface PnpVideoDevice {
  name: string
  manufacturer: string
  instanceId: string
  vendorId: string
  productId: string
  status: string
}

interface KbmNativeApi {
  mini: {
    keyboard: {
      capture(uid: string, enabled: boolean): Promise<MiniKeyboardState>
      onState(listener: (state: MiniKeyboardState) => void): () => void
    }
    tools: MiniToolsApi
    video: {
      start(uid: string, mode: MiniVideoMode, requestId: number): Promise<MiniVideoInfo>
      stop(): Promise<void>
      ack(token: number, sequence: number): void
      onFrame(listener: (frame: MiniVideoFrame) => void): () => void
      onEnded(listener: (value: { uid: string; message: string }) => void): () => void
    }
    scan(): Promise<MiniInventory>
    state(): Promise<MiniInventory>
    releaseInput(): Promise<void>
    input(uid: string, input: MiniInput): Promise<void>
    onInventory(listener: (value: MiniInventory) => void): () => void
  }
  hidDiagnostics: {
    state(): Promise<HidDiagnosticsState>
    onAlarm(listener: (alarm: HidAlarm) => void): () => void
  }
  input: {
    state(): Promise<[SpecialInputState, SpecialInputState]>
    chord(port: NetworkPort, request: KeyChordRequest): Promise<void>
    power(port: NetworkPort, confirmed: boolean): Promise<void>
    system(port: NetworkPort, usage: number, confirmed: boolean): Promise<SystemControlResult>
    release(port: NetworkPort): Promise<void>
  }
  wake: {
    adapters(): Promise<WolAdapter[]>
    wol(port: NetworkPort, config: WolConfig): Promise<WakeResult>
  }
  logs: {
    record(level: 'info' | 'warn' | 'error', event: string, fields: LogFields): void
    state(): Promise<LogState>
    configure(enabled: boolean, verbose: boolean): Promise<LogState>
    open(): Promise<void>
  }
  disk: {
    metrics(): Promise<[DiskTransferMetrics, DiskTransferMetrics]>
    choose(port: NetworkPort): Promise<string | null>
    prepare(port: NetworkPort): Promise<DiskSnapshotInfo>
    disable(port: NetworkPort): Promise<void>
    read(port: NetworkPort, request: DiskReadRequest): Promise<Uint8Array>
  }
  network: {
    state(): Promise<NetworkState>
    configure(config: NetworkConfig): Promise<void>
    enable(port: NetworkPort, enabled: boolean): Promise<void>
    forward(rules: ForwardRule[]): Promise<void>
    stopForward(): Promise<void>
  }
  master: {
    connect(): Promise<boolean>
    connected(): Promise<boolean>
    write(bytes: Uint8Array): Promise<void>
    onData(listener: (bytes: Uint8Array) => void): () => void
    onConnection(listener: (connected: boolean) => void): () => void
    onError(listener: (message: string) => void): () => void
  }
  video: {
    listPnp(): Promise<PnpVideoDevice[]>
  }
}

declare global {
  interface Window {
    kbmNative: KbmNativeApi
  }
}

export {}
