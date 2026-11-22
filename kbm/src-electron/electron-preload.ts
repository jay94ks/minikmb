import { contextBridge, ipcRenderer } from 'electron'
import { quasarRuntime } from '#q-app/electron/preload'
import type { MiniInput, MiniInventory, MiniKeyboardState } from '../src/shared/minikmb'
import type { MiniVideoInfo, MiniVideoFrame, MiniVideoMode } from '../src/shared/mini-capture'
import type { MiniToolsApi } from '../src/shared/mini-tools'
import type { NetworkConfig, NetworkPort, ForwardRule, NetworkState } from '../src/shared/network'
import type { DiskSnapshotInfo, DiskReadRequest, DiskTransferMetrics } from '../src/shared/disk'
import type { LogState, LogFields } from '../src/shared/logging'
import type { WolAdapter, WolConfig, WakeResult } from '../src/shared/wake'
import type { KeyChordRequest, SpecialInputState } from '../src/shared/special-input'
import type { SystemControlResult } from '../src/shared/system-controls'
import type { HidAlarm, HidDiagnosticsState } from '../src/shared/hid-diagnostics'

contextBridge.exposeInMainWorld('quasarRuntime', quasarRuntime)
contextBridge.exposeInMainWorld('kbmNative', {
  mini: {
    keyboard: {
      capture: (uid: string, enabled: boolean): Promise<MiniKeyboardState> => ipcRenderer.invoke('minikmb:keyboard-capture', uid, enabled),
      onState: (listener: (state: MiniKeyboardState) => void): (() => void) => {
        const handler = (_event: Electron.IpcRendererEvent, state: MiniKeyboardState) => listener(state)
        ipcRenderer.on('minikmb:keyboard-state', handler); return () => ipcRenderer.removeListener('minikmb:keyboard-state', handler)
      }
    },
    tools: {
      rename: (uid, name) => ipcRenderer.invoke('minikmb:rename', uid, name),
      state: uid => ipcRenderer.invoke('minikmb:tools-state', uid),
      terminal: uid => ipcRenderer.invoke('minikmb:terminal', uid),
      serialWrite: (uid, bytes) => ipcRenderer.invoke('minikmb:serial-write', uid, Array.from(bytes)),
      chord: (uid, request) => ipcRenderer.invoke('minikmb:chord', uid, { modifiers: request.modifiers, key: request.key }),
      system: (uid, usage, confirmed) => ipcRenderer.invoke('minikmb:system', uid, usage, confirmed),
      release: uid => ipcRenderer.invoke('minikmb:release', uid),
      deactivate: uid => ipcRenderer.invoke('minikmb:deactivate', uid),
      configure: (uid, config) => ipcRenderer.invoke('minikmb:configure', uid, { serverIp: config.serverIp, poolStart: config.poolStart, poolEnd: config.poolEnd, leaseSeconds: config.leaseSeconds }),
      network: (uid, mode) => ipcRenderer.invoke('minikmb:network', uid, mode),
      scan: (uid, request) => ipcRenderer.invoke('minikmb:port-scan', uid, request ? { target: request.target, start: request.start, end: request.end, timeoutMs: request.timeoutMs } : null),
      forward: (uid, rules) => ipcRenderer.invoke('minikmb:forward', uid, rules.map(r => ({ id: r.id, port: r.port, protocol: r.protocol, listenPort: r.listenPort, targetIp: r.targetIp, targetPort: r.targetPort }))),
      stopForward: uid => ipcRenderer.invoke('minikmb:stop-forward', uid),
      storage: (uid, action) => ipcRenderer.invoke('minikmb:storage', uid, { ...action })
    } satisfies MiniToolsApi,
    video: {
      start: (uid: string, mode: MiniVideoMode, requestId: number): Promise<MiniVideoInfo> => ipcRenderer.invoke('minikmb:video-start', uid, typeof mode === 'object' ? { width: mode.width, height: mode.height, frameRate: mode.frameRate } : mode, requestId),
      stop: (): Promise<void> => ipcRenderer.invoke('minikmb:video-stop'),
      ack: (token: number, sequence: number): void => ipcRenderer.send('minikmb:video-ack', token, sequence),
      onFrame: (listener: (frame: MiniVideoFrame) => void): (() => void) => {
        const handler = (_event: Electron.IpcRendererEvent, frame: MiniVideoFrame) => listener(frame)
        ipcRenderer.on('minikmb:video-frame', handler); return () => ipcRenderer.removeListener('minikmb:video-frame', handler)
      },
      onEnded: (listener: (value: { uid: string; message: string }) => void): (() => void) => {
        const handler = (_event: Electron.IpcRendererEvent, value: { uid: string; message: string }) => listener(value)
        ipcRenderer.on('minikmb:video-ended', handler); return () => ipcRenderer.removeListener('minikmb:video-ended', handler)
      }
    },
    scan: (): Promise<MiniInventory> => ipcRenderer.invoke('minikmb:scan'),
    state: (): Promise<MiniInventory> => ipcRenderer.invoke('minikmb:state'),
    releaseInput: (): Promise<void> => ipcRenderer.invoke('minikmb:release-input'),
    input: (uid: string, input: MiniInput): Promise<void> => ipcRenderer.invoke('minikmb:input', uid, input),
    onInventory: (listener: (value: MiniInventory) => void): (() => void) => {
      const handler = (_event: Electron.IpcRendererEvent, value: MiniInventory) => listener(value)
      ipcRenderer.on('minikmb:inventory', handler)
      return () => ipcRenderer.removeListener('minikmb:inventory', handler)
    }
  },
  hidDiagnostics: {
    state: (): Promise<HidDiagnosticsState> => ipcRenderer.invoke('kbm-hid:state'),
    onAlarm: (listener: (alarm: HidAlarm) => void): (() => void) => {
      const handler = (_event: Electron.IpcRendererEvent, alarm: HidAlarm): void => listener(alarm)
      ipcRenderer.on('kbm-hid:alarm',handler); return () => ipcRenderer.removeListener('kbm-hid:alarm',handler)
    }
  },
  input: {
    state: (): Promise<[SpecialInputState, SpecialInputState]> => ipcRenderer.invoke('kbm-input:state'),
    chord: (port: NetworkPort, request: KeyChordRequest): Promise<void> => ipcRenderer.invoke('kbm-input:chord', port, request),
    power: (port: NetworkPort, confirmed: boolean): Promise<void> => ipcRenderer.invoke('kbm-input:power', port, confirmed),
    system: (port: NetworkPort, usage: number, confirmed: boolean): Promise<SystemControlResult> => ipcRenderer.invoke('kbm-input:system', port, usage, confirmed),
    release: (port: NetworkPort): Promise<void> => ipcRenderer.invoke('kbm-input:release', port)
  },
  wake: {
    adapters: (): Promise<WolAdapter[]> => ipcRenderer.invoke('kbm-wake:adapters'),
    wol: (port: NetworkPort, config: WolConfig): Promise<WakeResult> => ipcRenderer.invoke('kbm-wake:wol', port, { enabled: config.enabled, mac: config.mac, localAddress: config.localAddress })
  },
  logs: {
    record: (level: 'info' | 'warn' | 'error', event: string, fields: LogFields): void => ipcRenderer.send('kbm-log:record', level, event, fields),
    state: (): Promise<LogState> => ipcRenderer.invoke('kbm-log:state'),
    configure: (enabled: boolean, verbose: boolean): Promise<LogState> => ipcRenderer.invoke('kbm-log:configure', enabled, verbose),
    open: (): Promise<void> => ipcRenderer.invoke('kbm-log:open')
  },
  disk: {
    metrics: (): Promise<[DiskTransferMetrics, DiskTransferMetrics]> => ipcRenderer.invoke('kbm-disk:metrics'),
    choose: (port: NetworkPort): Promise<string | null> => ipcRenderer.invoke('kbm-disk:choose', port),
    prepare: (port: NetworkPort): Promise<DiskSnapshotInfo> => ipcRenderer.invoke('kbm-disk:prepare', port),
    disable: (port: NetworkPort): Promise<void> => ipcRenderer.invoke('kbm-disk:disable', port),
    read: async (port: NetworkPort, request: DiskReadRequest): Promise<Uint8Array> =>
      Uint8Array.from(await ipcRenderer.invoke('kbm-disk:read', port, request) as number[])
  },
  network: {
    state: (): Promise<NetworkState> => ipcRenderer.invoke('kbm-network:state'),
    configure: (config: NetworkConfig): Promise<void> => ipcRenderer.invoke('kbm-network:configure', config),
    enable: (port: NetworkPort, enabled: boolean): Promise<void> => ipcRenderer.invoke('kbm-network:enable', port, enabled),
    forward: (rules: ForwardRule[]): Promise<void> => ipcRenderer.invoke('kbm-network:forward', rules),
    stopForward: (): Promise<void> => ipcRenderer.invoke('kbm-network:stop-forward')
  },
  master: {
    connect: (): Promise<boolean> => ipcRenderer.invoke('kbm-master:connect'),
    connected: (): Promise<boolean> => ipcRenderer.invoke('kbm-master:connected'),
    write: (bytes: Uint8Array): Promise<void> =>
      ipcRenderer.invoke('kbm-master:write', Array.from(bytes)),
    onData: (listener: (bytes: Uint8Array) => void): (() => void) => {
      const handler = (_event: Electron.IpcRendererEvent, bytes: number[]) => {
        listener(Uint8Array.from(bytes))
      }
      ipcRenderer.on('kbm-master:data', handler)
      return () => ipcRenderer.removeListener('kbm-master:data', handler)
    },
    onConnection: (listener: (connected: boolean) => void): (() => void) => {
      const handler = (_event: Electron.IpcRendererEvent, connected: boolean) => {
        listener(connected)
      }
      ipcRenderer.on('kbm-master:connection', handler)
      return () => ipcRenderer.removeListener('kbm-master:connection', handler)
    },
    onError: (listener: (message: string) => void): (() => void) => {
      const handler = (_event: Electron.IpcRendererEvent, message: string) => listener(message)
      ipcRenderer.on('kbm-master:error', handler)
      return () => ipcRenderer.removeListener('kbm-master:error', handler)
    }
  },
  video: {
    listPnp: (): Promise<unknown> => ipcRenderer.invoke('kbm-video:list-pnp')
  }
})
