import { encodePacket, Op, ProgressiveDecoder, u16le, type Packet } from './protocol'
import { EthernetAssembler, fragmentFrame } from './ethernet'
import { crc32 } from './ethernet'
import { isUint32, validateDiskToken, type DiskDeviceStatus, type DiskReadRequest } from '../shared/disk'
import { UsbWakeCode, type WakeResult } from '../shared/wake'
import { slaveBoot } from '../shared/boot'

export const MASTER_USB_VID = 0x8857
export const MASTER_USB_PID = 0x1223
export const MASTER_PROTOCOL_VERSION = 3
export const UART_BASELINE_BAUD = 921600
export type UartPort = 0 | 1
export interface NativeModeState { enabled: boolean; dhcp: boolean; visible: boolean; transition: number }
export interface NetworkDeviceStatus { enabled: boolean; ready: boolean; mac: string; advertisedBps: number }

export interface UartDiagnostics {
  txFrames: number
  rxBytes: number
  rxFrames: number
  decodeErrors: number
}
export interface MasterSnapshot {
  connected: boolean
  protocolVersion: number
  serialFlags: readonly [number, number]
  online: readonly [boolean, boolean]
  diagnostics: readonly [UartDiagnostics, UartDiagnostics]
  error: string
}
type StateListener = (snapshot: MasterSnapshot) => void
type SerialListener = (port: UartPort, bytes: Uint8Array) => void
interface QueuedRequest {
  packet: Packet
  expectedOp: number
  sequence: number | undefined
  resolve: (packet: Packet) => void
  reject: (error: Error) => void
  timer?: ReturnType<typeof setTimeout>
}
function clampI8(value: number): number {
  return Math.max(-127, Math.min(127, Math.round(value)))
}

export class MasterSerialClient {
  private initialized = false
  private connecting: Promise<boolean> | undefined
  private decoder = new ProgressiveDecoder()
  private listeners = new Set<StateListener>()
  private serialListeners = new Set<SerialListener>()
  private bootListeners = new Set<(port: UartPort) => void>()
  private ethernetListeners = new Set<SerialListener>()
  private diskListeners = new Set<(port: UartPort, request: DiskReadRequest) => void>()
  private ethernetAssemblers = [new EthernetAssembler(), new EthernetAssembler()]
  private frameId = 0
  private ethernetBusy = [false, false]
  private testingPorts = [false, false]
  private rebootingPorts = [false, false]
  private nativeActive = false
  private nativeSupported: boolean | undefined
  // UART0, UART1, local STATUS: independent stop-and-wait channels.
  private queues: QueuedRequest[][] = [[], [], []]
  private bulkQueues: QueuedRequest[][] = [[], [], []]
  private active: Array<QueuedRequest | undefined> = [undefined, undefined, undefined]
  private sequence = 0
  private statusTimer: ReturnType<typeof setInterval> | undefined
  private state: MasterSnapshot = {
    connected: false, protocolVersion: 0, online: [false, false], serialFlags: [0, 0],
    diagnostics: [
      { txFrames: 0, rxBytes: 0, rxFrames: 0, decodeErrors: 0 },
      { txFrames: 0, rxBytes: 0, rxFrames: 0, decodeErrors: 0 }
    ], error: ''
  }

  subscribe(listener: StateListener): () => void {
    this.listeners.add(listener)
    listener(this.state)
    return () => this.listeners.delete(listener)
  }
  onSerialData(listener: SerialListener): () => void {
    this.serialListeners.add(listener)
    return () => this.serialListeners.delete(listener)
  }
  onSlaveBoot(listener: (port: UartPort) => void): () => void {
    this.bootListeners.add(listener); return () => this.bootListeners.delete(listener)
  }
  onEthernetFrame(listener: SerialListener): () => void {
    this.ethernetListeners.add(listener)
    return () => this.ethernetListeners.delete(listener)
  }
  onDiskRead(listener: (port: UartPort, request: DiskReadRequest) => void): () => void {
    this.diskListeners.add(listener); return () => this.diskListeners.delete(listener)
  }
  diskConfigure(port: UartPort, generation: number, sectors: number, enabled: boolean): Promise<Packet> {
    if (!isUint32(generation) || !generation || !Number.isInteger(sectors) ||
        sectors < (enabled ? 1 : 0) || sectors > 67108864 || (!enabled && sectors !== 0)) {
      return Promise.reject(new Error('Invalid disk configuration'))
    }
    const payload = new Uint8Array(9), view = new DataView(payload.buffer)
    payload[0] = Number(enabled); view.setUint32(1, generation, true); view.setUint32(5, enabled ? sectors : 0, true)
    return this.route(port, { op: Op.DiskConfig, payload })
  }
  async diskStatus(port: UartPort): Promise<DiskDeviceStatus> {
    const response = await this.route(port, { op: Op.DiskStatus, payload: new Uint8Array() })
    const flags = response.payload[1] ?? 0
    if (response.payload.length !== 2 || (flags & ~3)) throw new Error('스토리지 상태 응답이 잘못되었습니다.')
    return { configured: Boolean(flags & 1), ready: Boolean(flags & 2) }
  }
  async networkStatus(port: UartPort): Promise<NetworkDeviceStatus> {
    const response = await this.route(port, { op: Op.NetStatus, payload: new Uint8Array() })
    const p = response.payload, flags = p[1] ?? 0
    if (p.length !== 12 || (flags & ~3)) throw new Error('NCM 상태 응답이 잘못되었습니다.')
    const view = new DataView(p.buffer, p.byteOffset, p.byteLength)
    return { enabled: Boolean(flags & 2), ready: Boolean(flags & 1),
      mac: Array.from(p.subarray(2, 8), n => n.toString(16).padStart(2, '0')).join(':'),
      advertisedBps: view.getUint32(8, true) }
  }
  diskFail(port: UartPort, request: DiskReadRequest): Promise<Packet> {
    validateDiskToken(request)
    const payload = new Uint8Array(12), view = new DataView(payload.buffer)
    view.setUint32(0, request.generation, true); view.setUint32(4, request.id, true); view.setUint32(8, request.lba, true)
    return this.route(port, { op: Op.DiskFail, payload })
  }
  async diskData(port: UartPort, request: DiskReadRequest, bytes: Uint8Array, current: () => boolean): Promise<void> {
    validateDiskToken(request)
    if (bytes.length !== (request.sectors ?? 1)*512 || bytes.length>4096) throw new Error('Disk data must match1..8 sectors')
    const crc = crc32(bytes)
    for (let offset = 0; offset < bytes.length && current(); offset += 4096) {
      const payload = new Uint8Array(18+Math.min(4096,bytes.length-offset)), view = new DataView(payload.buffer)
      view.setUint32(0, request.generation, true); view.setUint32(4, request.id, true); view.setUint32(8, request.lba, true)
      view.setUint16(12, offset, true); view.setUint32(14, crc, true); payload.set(bytes.subarray(offset, offset + 4096), 18)
      await this.route(port, { op: Op.DiskData, payload })
    }
  }
  async ethernetWrite(port: UartPort, frame: Uint8Array): Promise<void> {
    if (this.ethernetBusy[port]) throw new Error('Ethernet frame already in flight')
    this.ethernetBusy[port] = true
    try {
      for (const payload of fragmentFrame(frame, ++this.frameId)) {
        await this.route(port, { op: 0x40, payload })
      }
    } finally { this.ethernetBusy[port] = false }
  }
  networkEnable(port: UartPort, enabled: boolean): Promise<Packet> {
    return this.route(port, { op: Op.NetEnable, payload: Uint8Array.of(Number(enabled)) })
  }
  isRebooting(port: UartPort): boolean { return this.rebootingPorts[port] ?? false }
  async wakePc(port: UartPort): Promise<WakeResult> {
    const response = await this.route(port, { op: Op.WakePc, payload: new Uint8Array() })
    // Older firmware reports BAD_OPCODE; normalize ONLY that capability outcome,
    // never timeouts, bridge failures or malformed responses.
    const code = response.payload[0] === 1 ? UsbWakeCode.IgnoredUnsupported : response.payload[1]
    if (response.payload.length !== 2 || code === undefined || code > 6) throw new Error('USB 깨우기 응답이 잘못되었습니다.')
    window.kbmNative.logs?.record('info', 'ui.wake.usb_result', { port, code })
    return { code }
  }
  async queryNativeMode(): Promise<NativeModeState | undefined> {
    if (this.nativeSupported === false) return undefined
    try { return this.readNativeMode(await this.enqueue(2, { op: Op.NativeMode, payload: new Uint8Array() }, 0xf7)) }
    catch (error) {
      if (error instanceof Error && /status (1|7)$/.test(error.message)) { this.nativeSupported = false; return undefined }
      throw error
    }
  }
  async setNativeMode(enabled: boolean, dhcp: boolean): Promise<NativeModeState> {
    const response = await this.enqueue(2, { op: Op.NativeMode, payload: Uint8Array.of(Number(enabled), Number(dhcp)) }, 0xf7)
    return this.readNativeMode(response)
  }
  private readNativeMode(response: Packet): NativeModeState {
    if (response.payload.length !== 5) throw new Error('네이티브 NCM 펌웨어 응답이 잘못되었습니다.')
    const state = { enabled: response.payload[1] === 1, dhcp: response.payload[2] === 1,
      visible: response.payload[3] === 1, transition: response.payload[4] ?? 0 }
    if (response.payload[1]! > 1 || response.payload[2]! > 1 || response.payload[3]! > 1 || state.transition > 3) {
      throw new Error('네이티브 NCM 상태 값이 잘못되었습니다.')
    }
    this.nativeSupported = true; this.nativeActive = state.enabled || state.visible || state.transition !== 0
    return state
  }
  async rebootSlave(port: UartPort): Promise<void> {
    if (this.rebootingPorts[port] || this.testingPorts[port]) throw new Error('해당 보드 작업이 진행 중입니다.')
    await this.releaseAll(port)
    this.rebootingPorts[port] = true
    try {
      await window.kbmNative.network.enable(port, false)
      // Exactly once. A lost ACK does not authorize another reset.
      await this.route(port, { op: Op.RebootSlave, payload: Uint8Array.of(0x52, 0x50, 0x32, 0x30) })
      await new Promise(resolve => setTimeout(resolve, 1500))
      await this.route(port, { op: Op.Ping, payload: new Uint8Array() })
    } finally { this.rebootingPorts[port] = false }
  }
  isLinkTesting(port: UartPort): boolean { return this.testingPorts[port] ?? false }
  async testBaudRates(port: UartPort,
    progress: (message: string) => void): Promise<Array<{ baud: number; passed: number; errors: number; elapsedMs: number }>> {
    if (this.testingPorts.some(Boolean)) throw new Error('UART 시험이 이미 실행 중입니다.')
    if (this.nativeActive) throw new Error('네이티브 NCM 모드를 먼저 끄고 전환 완료를 기다리세요.')
    const disk = await this.diskStatus(port)
    if (disk.ready) throw new Error('가상 디스크 공유를 먼저 끄고 UART 속도 시험을 시작하세요.')
    // Stop background frame producers BEFORE entering the temporary speed lease.
    await window.kbmNative.network.enable(port, false)
    await this.networkEnable(port, false)
    this.testingPorts[port] = true
    const results: Array<{ baud: number; passed: number; errors: number; elapsedMs: number }> = []
    const wait = (ms: number): Promise<void> => new Promise(resolve => setTimeout(resolve, ms))
    try {
      for (const baud of [115200, 230400, 460800, 921600, 1000000]) {
        progress(`${baud.toLocaleString()} bps: 64개 패턴 왕복 시험`)
        const lease = 10000, start = performance.now()
        let passed = 0, errors = 0, switched = false
        try {
          const payload = new Uint8Array(6), view = new DataView(payload.buffer)
          view.setUint32(0, baud, true); view.setUint16(4, lease, true)
          await this.route(port, { op: 0x50, payload }); switched = true
          await wait(100)
          for (let n = 0; n < 64 && performance.now() - start < 7500; ++n) {
            const pattern = Uint8Array.from({ length: 248 }, (_, i) => {
              if (n % 4 === 0) return [0x55, 0xaa, 0x00, 0xff][i % 4]!
              if (n % 4 === 1) return [2, 3, 27][i % 3]!
              return (i * 73 + n * 151 + (i >>> 1)) & 255
            })
            try {
              const response = await this.route(port, { op: 0x51, payload: pattern })
              if (response.payload.length === 249 && response.payload.subarray(1).every((b, i) => b === pattern[i])) ++passed
              else ++errors
            } catch { ++errors; break } // Never retry a failed command at an uncertain speed.
          }
        } catch { ++errors }
        const elapsedMs = Math.round(performance.now() - start)
        if (passed !== 64 && errors === 0) ++errors
        results.push({ baud, passed, errors, elapsedMs })
        progress(`${baud.toLocaleString()} bps: ${passed}/64 성공, 오류 ${errors} · ${UART_BASELINE_BAUD} 복귀 대기`)
        // Even a lost SET ACK might have switched the slave. Always wait the
        // full bounded lease. Both boards recover without a PC command.
        await wait(Math.max(0, lease + 400 - (performance.now() - start)))
        if (!switched && passed === 0) {
          progress('속도 시험 명령이 실패했습니다. 두 보드 펌웨어 버전을 확인하세요.')
          break
        }
      }
      progress(`시험 완료 · 기본 ${UART_BASELINE_BAUD} bps 복귀. 짧은 시험 결과이며 영구 속도는 변경하지 않았습니다.`)
      return results
    } finally { this.testingPorts[port] = false }
  }
  async initialize(): Promise<boolean> {
    if (!this.initialized) {
      this.initialized = true
      window.kbmNative.master.onData(bytes => this.handleBytes(bytes))
      window.kbmNative.master.onConnection(connected => {
        if (connected) void this.connect()
        else this.handleDisconnected()
      })
      window.kbmNative.master.onError(message => this.setState({ error: message }))
    }
    return this.connect()
  }
  async connect(): Promise<boolean> {
    if (this.connecting) return this.connecting
    if (this.state.connected && this.state.protocolVersion === MASTER_PROTOCOL_VERSION) return true
    this.setState({ error: '' })
    this.connecting = (async () => {
      if (!await window.kbmNative.master.connect()) return false
      this.decoder = new ProgressiveDecoder()
      this.setState({ connected: true, protocolVersion: 0, error: '' })
      await this.requestStatus()
      this.nativeSupported = undefined
      await this.queryNativeMode().catch(() => undefined)
      this.startStatusPolling()
      return true
    })().catch(error => {
      this.handleDisconnected()
      this.setState({ error: this.messageOf(error) })
      return false
    }).finally(() => { this.connecting = undefined })
    return this.connecting
  }
  async requestStatus(): Promise<void> {
    const response = await this.enqueue(2, { op: Op.Status, payload: new Uint8Array() }, Op.Status | 0x80)
    if (response.payload.length !== 21 || response.payload[1] !== MASTER_PROTOCOL_VERSION) {
      throw new Error('Master 펌웨어를 프로토콜 v3로 업데이트하세요.')
    }
    const mask = response.payload[2] ?? 0
    const readU16 = (at: number): number =>
      (response.payload[at] ?? 0) | ((response.payload[at + 1] ?? 0) << 8)
    const diagnostics = [0, 1].map(port => {
      const offset = 3 + port * 8
      return { txFrames: readU16(offset), rxBytes: readU16(offset + 2),
        rxFrames: readU16(offset + 4), decodeErrors: readU16(offset + 6) }
    }) as [UartDiagnostics, UartDiagnostics]
    this.setState({ protocolVersion: MASTER_PROTOCOL_VERSION,
      online: [(mask & 1) !== 0, (mask & 2) !== 0], diagnostics,
      serialFlags: [response.payload[19] ?? 0, response.payload[20] ?? 0] })
  }
  keyDown(port: UartPort, usage: number): Promise<Packet> {
    return this.route(port, { op: Op.KeyDown, payload: Uint8Array.of(usage) })
  }
  keyUp(port: UartPort, usage: number): Promise<Packet> {
    return this.route(port, { op: Op.KeyUp, payload: Uint8Array.of(usage) })
  }
  mouseMove(port: UartPort, x: number, y: number, wheel = 0, pan = 0): Promise<Packet> {
    const [xl, xh] = u16le(x)
    const [yl, yh] = u16le(y)
    return this.route(port, { op: Op.MouseMove,
      payload: Uint8Array.of(xl, xh, yl, yh, clampI8(wheel) & 0xff, clampI8(pan) & 0xff) })
  }
  mouseDown(port: UartPort, buttons: number): Promise<Packet> {
    return this.route(port, { op: Op.MouseDown, payload: Uint8Array.of(buttons) })
  }
  mouseUp(port: UartPort, buttons: number): Promise<Packet> {
    return this.route(port, { op: Op.MouseUp, payload: Uint8Array.of(buttons) })
  }
  releaseAll(port: UartPort): Promise<Packet> {
    return this.route(port, { op: Op.ReleaseAll, payload: new Uint8Array() })
  }
  serialStatus(port: UartPort): Promise<Packet> {
    return this.route(port, { op: Op.SerialStatus, payload: new Uint8Array() })
  }
  async serialWrite(port: UartPort, bytes: Uint8Array): Promise<void> {
    if ((this.state.serialFlags[port] & 7) !== 7) {
      throw new Error(`UART${port}: slave COM 포트를 열고 DTR/RTS를 켜세요.`)
    }
    for (let offset = 0; offset < bytes.length; offset += 251) {
      await this.route(port, { op: Op.SerialWrite, payload: bytes.slice(offset, offset + 251) })
    }
  }
  route(port: UartPort, packet: Packet): Promise<Packet> {
    if (this.rebootingPorts[port] && packet.op !== 0x60 && packet.op !== Op.Ping) {
      return Promise.reject(new Error(`UART${port}: RP2040 재부팅 중입니다.`))
    }
    if (this.nativeActive && (packet.op === 0x40 || packet.op === 0x43 || packet.op === 0x50)) {
      return Promise.reject(new Error('네이티브 NCM 모드가 네트워크를 소유하고 있습니다.'))
    }
    if (this.testingPorts[port] && packet.op !== 0x50 && packet.op !== 0x51) {
      return Promise.reject(new Error(`UART${port}: 속도 시험 중입니다.`))
    }
    if ((port !== 0 && port !== 1) || this.state.protocolVersion !== MASTER_PROTOCOL_VERSION) {
      return Promise.reject(new Error('Master protocol v3 / UART0 또는 UART1이 필요합니다.'))
    }
    if (packet.payload.length > (packet.op===Op.DiskData ? 4114 : 251) || packet.op === 0x32 || packet.op === 0x42 ||
        (packet.op >= 0x70 && packet.op !== Op.ReleaseAll)) {
      return Promise.reject(new Error('Invalid routed command'))
    }
    const sequence = this.sequence = (this.sequence + 1) & 0xffff
    return this.enqueue(port, { op: Op.Route,
      payload: Uint8Array.of(port, sequence & 0xff, sequence >>> 8, packet.op, ...packet.payload)
    }, packet.op | 0x80, sequence, packet.op === 0x40 || packet.op === 0x45)
  }
  private enqueue(channel: number, packet: Packet, expectedOp: number, sequence?: number, bulk = false): Promise<Packet> {
    if (!this.state.connected) return Promise.reject(new Error('Master is not connected'))
    return new Promise((resolve, reject) => {
      ;(bulk ? this.bulkQueues : this.queues)[channel]!.push({ packet, expectedOp, sequence, resolve, reject })
      void this.pump(channel)
    })
  }
  private async pump(channel: number): Promise<void> {
    if (this.active[channel] || !this.state.connected) return
    const next = this.queues[channel]!.shift() ?? this.bulkQueues[channel]!.shift()
    if (!next) return
    this.active[channel] = next
    // Arm before write: a fast ACK may precede resolution of the write promise.
    next.timer = setTimeout(() => {
      if (this.active[channel] === next) this.finish(channel, undefined,
        new Error(channel < 2 ? `UART${channel} response timeout` : 'Master response timeout'))
    }, 1500)
    try {
      await window.kbmNative.master.write(encodePacket(next.packet))
    } catch (error) {
      if (this.active[channel] === next) this.finish(channel, undefined, new Error(this.messageOf(error)))
    }
  }
  private handleBytes(bytes: Uint8Array): void {
    for (const event of this.decoder.feed(bytes)) {
      if (event.type === 'packet') this.handlePacket(event.packet)
      else this.setState({ error: `Packet ${event.error} error` })
    }
  }
  private handlePacket(packet: Packet): void {
    const boot = slaveBoot(packet)
    if (boot) {
      const { port } = boot, error = new Error(`UART${port}: slave 재부팅으로 이전 요청이 취소되었습니다.`)
      const active = this.active[port]
      this.active[port] = undefined
      if (active?.timer) clearTimeout(active.timer)
      active?.reject(error)
      for (const queue of [this.queues[port]!, this.bulkQueues[port]!]) for (const request of queue.splice(0)) request.reject(error)
      this.ethernetAssemblers[port] = new EthernetAssembler()
      const online: [boolean, boolean] = [...this.state.online], serialFlags: [number, number] = [...this.state.serialFlags]
      online[port] = false; serialFlags[port] = 0
      this.setState({ online, serialFlags, error: error.message })
      for (const listener of this.bootListeners) listener(port)
      return
    }
    if (packet.op === Op.DiskRead && packet.payload.length === 15) {
      const port = packet.payload[0]
      if (port === 0 || port === 1) {
        const view = new DataView(packet.payload.buffer, packet.payload.byteOffset, packet.payload.byteLength)
        const request = { generation: view.getUint32(1, true), id: view.getUint32(5, true), lba: view.getUint32(9, true), sectors: view.getUint16(13, true) }
        if (!request.generation || !request.id || request.sectors<1 || request.sectors>8) return
        for (const listener of this.diskListeners) listener(port, request)
      }
      return
    }
    if (packet.op === 0xf6 && packet.payload.length > 11) {
      const port = packet.payload[0]
      if (port === 0 || port === 1) {
        const frame = this.ethernetAssemblers[port]!.accept(packet.payload.subarray(1))
        if (frame) for (const listener of this.ethernetListeners) listener(port, frame)
      }
      return
    }
    if (packet.op === Op.SerialState && packet.payload.length === 2) {
      const port = packet.payload[0]
      if (port === 0 || port === 1) this.applySerialFlags(port, packet.payload[1] ?? 0)
      return
    }
    if (packet.op === Op.SerialData && packet.payload.length >= 4) {
      const port = packet.payload[0]
      if (port === 0 || port === 1) {
        // bytes 1..2 are the slave transport sequence, already deduplicated by master.
        for (const listener of this.serialListeners) listener(port, packet.payload.slice(3))
      }
      return
    }
    if (packet.op === Op.OnlineChanged && packet.payload.length === 2) {
      const port = packet.payload[0]
      if (port === 0 || port === 1) {
        const online: [boolean, boolean] = [...this.state.online]
        online[port] = packet.payload[1] === 1
        this.setState({ online })
      }
      return
    }
    if (packet.op === (Op.Route | 0x80) && packet.payload.length >= 5) {
      const port = packet.payload[0]
      if (port !== 0 && port !== 1) return
      const sequence = (packet.payload[1] ?? 0) | ((packet.payload[2] ?? 0) << 8)
      const active = this.active[port]
      if (!active || active.sequence !== sequence) return
      const bridgeStatus = packet.payload[3]
      if (bridgeStatus !== 0) {
        this.finish(port, undefined, new Error(bridgeStatus === 0x0b ? `UART${port}: slave 재부팅으로 이전 요청이 취소되었습니다.` : `UART${port} bridge status ${bridgeStatus}`))
        return
      }
      if (packet.payload[4] !== active.expectedOp || packet.payload.length < 7) return
      if (packet.payload[4] === (Op.SerialStatus | 0x80) && packet.payload[5] === 0) {
        this.applySerialFlags(port, packet.payload[6] ?? 0)
      }
      this.complete(port, { op: packet.payload[4], payload: packet.payload.slice(5) })
    } else if (packet.op === this.active[2]?.expectedOp) {
      this.complete(2, packet)
    }
  }
  private applySerialFlags(port: UartPort, flags: number): void {
    const serialFlags: [number, number] = [...this.state.serialFlags]
    serialFlags[port] = flags
    this.setState({ serialFlags })
  }
  private complete(channel: number, packet: Packet): void {
    if (channel < 2 && packet.op === (Op.WakePc | 128) && packet.payload[0] === 1) {
      this.finish(channel, packet); return // Legacy unsupported is a non-error result.
    }
    if (packet.payload[0] !== 0) {
      this.finish(channel, undefined, new Error(`Channel ${channel} rejected command: status ${packet.payload[0]}`))
    } else this.finish(channel, packet)
  }
  private finish(channel: number, packet?: Packet, error?: Error): void {
    const active = this.active[channel]
    if (!active) return
    if (active.timer) clearTimeout(active.timer)
    this.active[channel] = undefined
    if (error) active.reject(error)
    else if (packet) active.resolve(packet)
    void this.pump(channel)
  }
  private startStatusPolling(): void {
    if (this.statusTimer) clearInterval(this.statusTimer)
    this.statusTimer = setInterval(() => {
      if (this.state.connected && !this.active[2] && this.queues[2]!.length === 0) {
        void this.requestStatus().catch(error => this.setState({ error: this.messageOf(error) }))
      }
    }, 1000)
  }
  private handleDisconnected(): void {
    if (this.statusTimer) clearInterval(this.statusTimer)
    this.statusTimer = undefined
    for (let channel = 0; channel < 3; ++channel) {
      const active = this.active[channel]
      if (active?.timer) clearTimeout(active.timer)
      active?.reject(new Error('Master disconnected'))
      this.active[channel] = undefined
      for (const request of this.queues[channel]!.splice(0)) request.reject(new Error('Master disconnected'))
      for (const request of this.bulkQueues[channel]!.splice(0)) request.reject(new Error('Master disconnected'))
    }
    this.setState({ connected: false, protocolVersion: 0, online: [false, false], serialFlags: [0, 0] })
  }
  private setState(patch: Partial<MasterSnapshot>): void {
    if (patch.error && patch.error !== this.state.error) window.kbmNative.logs?.record('error', 'ui.master_error', { error: patch.error })
    this.state = { ...this.state, ...patch }
    for (const listener of this.listeners) listener(this.state)
  }
  private messageOf(error: unknown): string {
    return error instanceof Error ? error.message : String(error)
  }
}
export const masterSerial = new MasterSerialClient()
