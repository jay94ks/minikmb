import { app, BrowserWindow, ipcMain, Menu, session, dialog, shell } from 'electron'
import { execFile } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { existsSync } from 'node:fs'
import { promisify } from 'node:util'
import { SerialPort as NativeSerialPort } from 'serialport'
import { registerQuasarRuntime } from '#q-app/electron/main'
import { NetworkService } from './network-service'
import type { NetworkConfig, NetworkPort, ForwardRule } from '../src/shared/network'
import { DiskService } from './disk-service'
import type { DiskReadRequest } from '../src/shared/disk'
import { encodePacket, Op, ProgressiveDecoder } from '../src/services/protocol'
import { RoutedTransport } from './routed-transport'
import { DiskTransfer } from './disk-transfer'
import { EthernetTransport } from './ethernet-transport'
import { OperationLogger } from './operation-log'
import { WakeOnLan } from './wake-on-lan'
import type { WolConfig } from '../src/shared/wake'
import { SpecialInput, SystemControlRejected } from './special-input'
import type { SystemControlResult } from '../src/shared/system-controls'
import { slaveBoot } from '../src/shared/boot'
import { HidDiagnostics } from './hid-diagnostics'
import { MiniService } from './minikmb-service'
import { MiniCapture } from './minikmb-capture'
import { MiniKeyboard } from './minikmb-keyboard'
import { MiniFeatures } from './minikmb-features'
import type { MiniStorageAction, MiniNetworkMode } from '../src/shared/mini-tools'
import type { PortScanRequest } from '../src/shared/network'
import type { MiniVideoMode } from '../src/shared/mini-capture'
import type { KeyChordRequest } from '../src/shared/special-input'
import type { LogFields } from '../src/shared/logging'
import { prepareUiBenchmark, runUiBenchmark } from './ui-benchmark'
// Parse before logger/profile construction and before any diagnostic probe.
const uiBenchmark = prepareUiBenchmark(process.argv.slice(app.isPackaged ? 1 : 2))
const operationLogger = new OperationLogger(path.join(app.getPath('userData'), 'logs'))
const log = operationLogger.log
const hidDiagnostics = new HidDiagnostics(log, alarm => broadcast('kbm-hid:alarm', alarm))
const miniService = new MiniService(snapshot => { miniFeatures.observe(snapshot); miniKeyboard.check(); broadcast('minikmb:inventory', snapshot) })
const miniFeatures = new MiniFeatures(miniService, log, async type => {
  const selection = await dialog.showOpenDialog({ title: type === 'folder' ? 'SD 스냅샷으로 공유할 폴더' : type === 'optical' ? '부팅 ISO/UDF 이미지' : '포맷된 FAT32/exFAT/EXT4 이미지',
    properties: type === 'folder' ? ['openDirectory'] : ['openFile'],
    filters: type === 'folder' ? [] : [{ name: 'Disk images', extensions: type === 'optical' ? ['iso', 'udf'] : ['img', 'bin', 'raw'] }] })
  return selection.canceled ? undefined : selection.filePaths[0]
})
const miniCaptureHelper = [path.join(process.resourcesPath, 'minikmb-capture.exe'),
  path.join(process.cwd(), 'src-electron/native-bin/minikmb-capture.exe'),
  path.join(app.getAppPath(), 'src-electron/native-bin/minikmb-capture.exe')].find(existsSync) ?? ''
const miniCapture = new MiniCapture(miniCaptureHelper, frame => broadcast('minikmb:video-frame', frame),
  (uid, message) => broadcast('minikmb:video-ended', { uid, message }))
const miniKeyboardHelper = [path.join(process.resourcesPath, 'minikmb-keyboard.exe'),
  path.join(process.cwd(), 'src-electron/native-bin/minikmb-keyboard.exe'),
  path.join(app.getAppPath(), 'src-electron/native-bin/minikmb-keyboard.exe')].find(existsSync) ?? ''
const miniKeyboard = new MiniKeyboard(miniKeyboardHelper, uid => {
  try { const channel = miniService.channel(uid); return Boolean(BrowserWindow.getFocusedWindow()) && miniService.isSelected(uid) && Boolean(channel.features & 1) && [1, 2].includes(channel.targetState) }
  catch { return false }
}, async (uid, input, current) => { await miniFeatures.prepareInput(uid); if (current()) await miniService.input(uid, input) },
  uid => miniService.releaseInputFor(uid), state => broadcast('minikmb:keyboard-state', state))
const wakeOnLan = new WakeOnLan(log)
const diskService = new DiskService()
const routedTransport = new RoutedTransport(bytes => writeMasterRaw(bytes), log)
let masterProtocol = 0, onlineMask = 0
const specialInput = new SpecialInput(async (port, op, payload) => {
  if (!masterPort?.isOpen || masterProtocol !== 3 || !(onlineMask & (1 << port))) throw new Error('대상 UART 연결이 끊어졌습니다.')
  const reply = await routedTransport.request({ op: Op.Route, payload: Uint8Array.of(port, 0, 0, op, ...payload) })
  if ((op === 0x12 || op === 0x13) && reply.payload[3] === 0 && [1, 3].includes(reply.payload[5] ?? -1)) throw new SystemControlRejected('unsupported', 'slave 펌웨어가 이 System Control 키를 지원하지 않습니다. 새 펌웨어가 필요합니다.')
  if (op === 0x12 && reply.payload[3] === 0 && reply.payload[5] === 7) throw new SystemControlRejected('not-ready', '대상 USB가 활성 상태가 아닙니다. 절전 중에는 PC 깨우기를 사용하세요.')
  if (reply.payload[3] || reply.payload[5]) throw new Error(`특수 입력 응답 오류: bridge=${reply.payload[3]}, slave=${reply.payload[5]}`)
}, port => Boolean(masterPort?.isOpen && masterProtocol === 3 && (onlineMask & (1 << port))), log)
const diskTransfer = new DiskTransfer(diskService, routedTransport, undefined, log)
let masterDecoder = new ProgressiveDecoder()
const diskFolders: [string, string] = ['', '']

const platform = process.platform || os.platform()
const MASTER_VID = '8857'
const MASTER_PID = '1223'
const SERIAL_SCAN_INTERVAL_MS = 1_000
const execFileAsync = promisify(execFile)

let masterPort: NativeSerialPort | undefined
let masterConnecting: Promise<boolean> | undefined
let serialScanTimer: ReturnType<typeof setInterval> | undefined
let diagnosticsTimer: ReturnType<typeof setInterval> | undefined
let hidDiagnosticsTimer: ReturnType<typeof setInterval> | undefined
let previousUartErrors: number[] | undefined
const ethernetTransport = new EthernetTransport(routedTransport,
  (port, bytes) => networkService.receive(port, Array.from(bytes)))
const networkService = new NetworkService((port, bytes) => ethernetTransport.send(port, bytes), log)
let networkOperation = Promise.resolve()
function serializeNetwork(action: () => Promise<void>): Promise<void> {
  const result = networkOperation.then(action).catch(error => {
    log('error', 'network.operation_failed', { error: error instanceof Error ? error.message : String(error) })
    throw error
  })
  networkOperation = result.catch(() => {})
  return result
}

interface PnpVideoDevice {
  name: string
  manufacturer: string
  instanceId: string
  vendorId: string
  productId: string
  status: string
}

function normalizeUsbId(value?: string): string {
  return (value ?? '').toLowerCase().replace(/^0x/, '').padStart(4, '0')
}

function broadcast(channel: string, ...args: unknown[]): void {
  for (const window of BrowserWindow.getAllWindows()) {
    if (!window.isDestroyed()) window.webContents.send(channel, ...args)
  }
}

async function ensureMasterConnected(): Promise<boolean> {
  if (masterPort?.isOpen) return true
  if (masterConnecting) return masterConnecting

  masterConnecting = (async () => {
    const ports = await NativeSerialPort.list()
    const info = ports.find(port =>
      normalizeUsbId(port.vendorId) === MASTER_VID &&
      normalizeUsbId(port.productId) === MASTER_PID
    )
    if (!info) return false

    const candidate = new NativeSerialPort({
      path: info.path,
      baudRate: 115200,
      dataBits: 8,
      stopBits: 1,
      parity: 'none',
      rtscts: false,
      autoOpen: false
    })

    candidate.on('data', (chunk: Buffer) => {
      for (const item of masterDecoder.feed(chunk)) {
        if (item.type !== 'packet') { log('error', 'usb.decode_failed', { error: item.error }); broadcast('kbm-master:error', `Packet ${item.error} error`); continue }
        const packet = item.packet, p = packet.payload
        const boot = slaveBoot(packet)
        if (boot) {
          const { port } = boot
          onlineMask &= ~(1 << port)
          specialInput.restarted(port)
          routedTransport.resetPort(port)
          diskService.disable(port); ethernetTransport.cancel(port)
          void serializeNetwork(() => networkService.enable(port, false)).catch(() => {})
          log('warn', 'uart.slave_boot', boot)
          hidDiagnostics.boot(port,boot.firmware)
          broadcast('kbm-master:data', Array.from(encodePacket(packet)))
          continue
        }
        if (hidDiagnostics.accept(packet)) continue
        if (routedTransport.accept(packet)) continue
        if (ethernetTransport.accept(packet)) continue
        if (packet.op === 0xf1 && p.length === 21) {
          masterProtocol = p[1] ?? 0; onlineMask = p[2] ?? 0
          hidDiagnostics.online(0,Boolean(onlineMask & 1)); hidDiagnostics.online(1,Boolean(onlineMask & 2))
          const view = new DataView(p.buffer, p.byteOffset, p.byteLength)
          const errors = [view.getUint16(9, true), view.getUint16(17, true)]
          for (const port of [0, 1]) {
            if (!previousUartErrors) log('info', 'uart.error_baseline', { port, cumulative: errors[port] })
            else if (errors[port] !== previousUartErrors[port]) log('warn', 'uart.decode_errors', {
              port, cumulative: errors[port], delta: (errors[port]! - previousUartErrors[port]!) & 65535 })
          }
          previousUartErrors = errors
        }
        if (packet.op === Op.OnlineChanged && p.length === 2 && (p[0] === 0 || p[0] === 1)) {
          onlineMask = p[1] === 1 ? onlineMask | (1 << p[0]) : onlineMask & ~(1 << p[0])
          log('info', 'uart.online', { port: p[0], online: p[1] === 1 })
          hidDiagnostics.online(p[0],p[1] === 1)
        }
        if (packet.op === Op.DiskRead) {
          if (p.length === 15 && (p[0] === 0 || p[0] === 1)) {
            const view = new DataView(p.buffer, p.byteOffset, p.byteLength)
            void diskTransfer.serve(p[0], { generation: view.getUint32(1, true), id: view.getUint32(5, true), lba: view.getUint32(9, true), sectors: view.getUint16(13, true) })
          }
          continue // Native serving only: never race a second renderer producer.
        }
        if (packet.op === Op.OnlineChanged && p.length === 2 && p[1] === 0 && (p[0] === 0 || p[0] === 1)) {
          const port = p[0]
          diskService.disable(port); ethernetTransport.cancel(port)
          void serializeNetwork(() => networkService.enable(port, false)).catch(() => {})
        }
        broadcast('kbm-master:data', Array.from(encodePacket(packet)))
      }
    })
    candidate.on('error', (error: Error) => {
      log('error', 'usb.serial_error', { error: error.message })
      broadcast('kbm-master:error', error.message)
    })
    candidate.on('close', () => {
      log('warn', 'usb.disconnected')
      hidDiagnostics.master(false)
      previousUartErrors = undefined
      masterProtocol = onlineMask = 0
      if (masterPort === candidate) masterPort = undefined
      routedTransport.reset(); ethernetTransport.reset(); masterDecoder = new ProgressiveDecoder()
      broadcast('kbm-master:connection', false)
      diskService.disable(0); diskService.disable(1)
      void serializeNetwork(() => networkService.stop())
    })

    await new Promise<void>((resolve, reject) => {
      candidate.open(error => error ? reject(error) : resolve())
    })
    masterPort = candidate
    hidDiagnostics.master(true)
    log('info', 'usb.connected', { com: info.path, vid: MASTER_VID, pid: MASTER_PID })
    broadcast('kbm-master:connection', true)
    return true
  })().catch(error => {
    log('error', 'usb.connect_failed', { error: error instanceof Error ? error.message : String(error) })
    broadcast('kbm-master:error', error instanceof Error ? error.message : String(error))
    return false
  }).finally(() => {
    masterConnecting = undefined
  })

  return masterConnecting
}

async function writeMaster(bytes: unknown): Promise<void> {
  if (!Array.isArray(bytes) || bytes.length === 0 || bytes.length > 8242 ||
      bytes.some(value => !Number.isInteger(value) || value < 0 || value > 255)) {
    throw new Error('Invalid master serial frame')
  }
  const decoded = new ProgressiveDecoder().feed(Uint8Array.from(bytes))
  if (decoded.length !== 1 || decoded[0]?.type !== 'packet') throw new Error('Expected one complete master frame')
  const packet = decoded[0].packet
  if (packet.op === Op.Route && [0x43, 0x44, 0x60].includes(packet.payload[3] ?? -1)) {
    log('info', 'control.operation', { port: packet.payload[0], op: packet.payload[3],
      enabled: packet.payload[3] === 0x43 || packet.payload[3] === 0x44 ? packet.payload[4] === 1 : undefined })
  }
  if (packet.op === Op.NativeMode && packet.payload.length) log('info', 'network.native_mode', { enabled: packet.payload[0] === 1, dhcp: packet.payload[1] === 1 })
  if (packet.op === Op.Route) {
    const op = packet.payload[3]
    if (specialInput.blocked(packet.payload[0]!) && [0x10, 0x12, 0x20, 0x21, 0x50, 0x60].includes(op ?? -1)) {
      throw new Error('이 UART의 특수 입력 또는 미확인 키 해제가 진행 중입니다.')
    }
    const response = await routedTransport.request(packet, op === Op.NetWrite || op === Op.DiskData)
    broadcast('kbm-master:data', Array.from(encodePacket(response)))
  } else await writeMasterRaw(Uint8Array.from(bytes))
}

async function writeMasterRaw(bytes: Uint8Array): Promise<void> {
  if (!await ensureMasterConnected() || !masterPort?.isOpen) {
    throw new Error('Master is not connected')
  }

  const port = masterPort
  await new Promise<void>((resolve, reject) => {
    port.write(Buffer.from(bytes), error => {
      if (error) {
        reject(error)
        return
      }
      // Real master/slave ACK is the completion barrier. FlushFileBuffers on
      // every USB CDC fragment adds an unnecessary OS/worker-pool barrier.
      resolve()
    })
  })
}

async function listPnpVideoDevices(): Promise<PnpVideoDevice[]> {
  if (platform !== 'win32') return []
  const command = [
    "Get-CimInstance -ClassName Win32_PnPEntity",
    "Where-Object { $_.PNPClass -eq 'Camera' }",
    'Select-Object Name,Manufacturer,PNPDeviceID,Status',
    'ConvertTo-Json -Compress'
  ].join(' | ')

  try {
    const { stdout } = await execFileAsync('powershell.exe', [
      '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', command
    ], { windowsHide: true, timeout: 5_000 })
    if (!stdout.trim()) return []
    const parsed: unknown = JSON.parse(stdout)
    const records = Array.isArray(parsed) ? parsed : [parsed]
    return records.flatMap(record => {
      if (!record || typeof record !== 'object') return []
      const source = record as Record<string, unknown>
      const instanceId = String(source.PNPDeviceID ?? '')
      const usb = /VID_([0-9A-F]{4}).*PID_([0-9A-F]{4})/i.exec(instanceId)
      return [{
        name: String(source.Name ?? 'Video device'),
        manufacturer: String(source.Manufacturer ?? ''),
        instanceId,
        vendorId: usb?.[1]?.toLowerCase() ?? '',
        productId: usb?.[2]?.toLowerCase() ?? '',
        status: String(source.Status ?? '')
      }]
    })
  } catch {
    return []
  }
}

function configureDeviceAccess(): void {
  const ses = session.defaultSession
  ses.setPermissionCheckHandler((_webContents, permission) => permission === 'media')
  ses.setPermissionRequestHandler((_webContents, permission, callback) => {
    callback(permission === 'media')
  })
}

function registerIpc(): void {
  ipcMain.handle('minikmb:video-start', async (_event, uid: unknown, mode: MiniVideoMode, requestId: number) => {
    await miniKeyboard.deactivate()
    const channel = miniService.channel(uid)
    await miniService.selectInput(channel.uid)
    if (!channel.captureId) throw new Error(channel.pairingError)
    if (!miniCaptureHelper) throw new Error('Media Foundation 캡처 helper가 없습니다. 앱을 다시 빌드하세요.')
    return miniCapture.start(channel.uid, channel.captureId, mode, requestId)
  })
  ipcMain.handle('minikmb:video-stop', async () => { await miniKeyboard.deactivate(); await miniService.clearInput(); await miniCapture.stop() })
  ipcMain.handle('minikmb:keyboard-capture', async (event, uid: string, enabled: boolean) => {
    const window = BrowserWindow.fromWebContents(event.sender)
    if (!window || typeof enabled !== 'boolean') throw new Error('키보드 캡처 요청 오류')
    if (!enabled) return miniKeyboard.deactivate()
    if (!window.isFocused()) throw new Error('영상 창이 활성 상태가 아닙니다.')
    miniService.channel(uid)
    const handle = window.getNativeWindowHandle()
    return miniKeyboard.set(uid, true, handle.length === 8 ? handle.readBigUInt64LE().toString(16) : handle.readUInt32LE().toString(16))
  })
  ipcMain.on('minikmb:video-ack', (_event, token: unknown, sequence: unknown) => miniCapture.ack(token, sequence))
  ipcMain.handle('minikmb:scan', () => miniService.scan())
  ipcMain.handle('minikmb:state', () => miniService.state())
  ipcMain.handle('minikmb:release-input', () => miniService.releaseInput())
  ipcMain.handle('minikmb:input', async (_event, uid: string, input) => { await miniFeatures.prepareInput(uid); await miniService.input(uid, input) })
  ipcMain.handle('minikmb:tools-state', (_event, uid: string) => miniFeatures.state(uid))
  ipcMain.handle('minikmb:rename', (_event, uid: string, name: string) => miniFeatures.rename(uid, name))
  ipcMain.handle('minikmb:terminal', (_event, uid: string) => miniFeatures.terminal(uid))
  ipcMain.handle('minikmb:serial-write', (_event, uid: string, bytes: number[]) => miniFeatures.serialWrite(uid, bytes))
  ipcMain.handle('minikmb:chord', (_event, uid: string, request: KeyChordRequest) => miniFeatures.chord(uid, request))
  ipcMain.handle('minikmb:system', (_event, uid: string, usage: number, confirmed: boolean) => miniFeatures.system(uid, usage, confirmed))
  ipcMain.handle('minikmb:release', (_event, uid: string) => miniFeatures.release(uid))
  ipcMain.handle('minikmb:deactivate', (_event, uid: string) => miniFeatures.deactivate(uid))
  ipcMain.handle('minikmb:configure', (_event, uid: string, config: NetworkConfig) => miniFeatures.configure(uid, config))
  ipcMain.handle('minikmb:network', (_event, uid: string, mode: MiniNetworkMode) => miniFeatures.network(uid, mode))
  ipcMain.handle('minikmb:port-scan', (_event, uid: string, request: PortScanRequest | null) => miniFeatures.scan(uid, request))
  ipcMain.handle('minikmb:forward', (_event, uid: string, rules: ForwardRule[]) => miniFeatures.forward(uid, rules))
  ipcMain.handle('minikmb:stop-forward', (_event, uid: string) => miniFeatures.stopForward(uid))
  ipcMain.handle('minikmb:storage', (_event, uid: string, action: MiniStorageAction) => miniFeatures.storage(uid, action))
  ipcMain.handle('kbm-hid:state', () => hidDiagnostics.state())
  ipcMain.handle('kbm-input:state', () => specialInput.state())
  ipcMain.handle('kbm-input:chord', (_event, port: 0 | 1, request: KeyChordRequest) => specialInput.chord(port, request))
  ipcMain.handle('kbm-input:power', (_event, port: 0 | 1, confirmed: boolean) => specialInput.power(port, confirmed))
  ipcMain.handle('kbm-input:system', async (_event, port: 0 | 1, usage: number, confirmed: boolean): Promise<SystemControlResult> => {
    try {
      await specialInput.system(port, usage, confirmed)
      return { status: 'accepted', hostActionConfirmed: false, message: 'DOWN/UP 큐 수락 · 대상 OS 동작은 미확인' }
    } catch (error) {
      if (error instanceof SystemControlRejected) return { status: error.status, hostActionConfirmed: false, message: error.message }
      throw error
    }
  })
  ipcMain.handle('kbm-input:release', (_event, port: 0 | 1) => specialInput.release(port))
  ipcMain.handle('kbm-wake:adapters', () => wakeOnLan.adapters())
  ipcMain.handle('kbm-wake:wol', (_event, port: number, config: WolConfig) => wakeOnLan.send(port, config))
  ipcMain.handle('kbm-log:state', () => operationLogger.snapshot())
  ipcMain.on('kbm-log:record', (_event, level: unknown, event: unknown, fields: unknown) => {
    if (!['info', 'warn', 'error'].includes(String(level)) || typeof event !== 'string' ||
        !/^ui\.[a-zA-Z0-9_.:-]{1,64}$/.test(event) || !fields || typeof fields !== 'object' || Array.isArray(fields)) return
    log(level as 'info' | 'warn' | 'error', event, fields as LogFields)
  })
  ipcMain.handle('kbm-log:configure', async (_event, enabled: unknown, verbose: unknown) => {
    if (typeof enabled !== 'boolean' || typeof verbose !== 'boolean') throw new Error('Invalid logging settings')
    await operationLogger.configure(enabled, verbose)
    return operationLogger.snapshot()
  })
  ipcMain.handle('kbm-log:open', async () => {
    log('info', 'logging.open_directory'); await operationLogger.flush()
    const error = await shell.openPath(operationLogger.directory)
    if (error) throw new Error(error)
  })
  ipcMain.handle('kbm-disk:choose', async (_event, port: NetworkPort) => {
    if (port !== 0 && port !== 1) throw new Error('Invalid UART')
    const result = await dialog.showOpenDialog({ title: `UART ${port} 읽기 전용 공유 폴더`, properties: ['openDirectory'] })
    if (result.canceled || !result.filePaths[0]) return null
    diskFolders[port] = result.filePaths[0]; return diskFolders[port]
  })
  ipcMain.handle('kbm-disk:prepare', async (_event, port: NetworkPort) => {
    if ((port !== 0 && port !== 1) || !diskFolders[port]) throw new Error('공유 폴더를 먼저 선택하세요.')
    log('info', 'disk.preparing', { port })
    try {
      const info = await diskService.prepare(port, diskFolders[port])
      log('info', 'disk.prepared', { port, generation: info.generation, files: info.files, bytes: info.bytes, sectors: info.sectors })
      return info
    } catch (error) { log('error', 'disk.prepare_failed', { port, error: error instanceof Error ? error.message : String(error) }); throw error }
  })
  ipcMain.handle('kbm-disk:disable', (_event, port: NetworkPort) => { log('info', 'disk.disabled', { port }); diskService.disable(port) })
  ipcMain.handle('kbm-disk:read', async (_event, port: NetworkPort, request: DiskReadRequest) => Array.from(await diskService.read(port, request)))
  ipcMain.handle('kbm-disk:metrics', () => diskTransfer.snapshot())
  ipcMain.handle('kbm-master:connect', () => {
    if (!serialScanTimer) serialScanTimer = setInterval(() => void ensureMasterConnected(), SERIAL_SCAN_INTERVAL_MS)
    return ensureMasterConnected()
  })
  ipcMain.handle('kbm-master:connected', () => Boolean(masterPort?.isOpen))
  ipcMain.handle('kbm-master:write', (_event, bytes: unknown) => writeMaster(bytes))
  ipcMain.handle('kbm-video:list-pnp', () => listPnpVideoDevices())
  ipcMain.handle('kbm-network:state', () => networkService.snapshot())
  ipcMain.handle('kbm-network:configure', (_event, config: NetworkConfig) =>
    serializeNetwork(() => networkService.configure(config)))
  ipcMain.handle('kbm-network:enable', (_event, port: NetworkPort, enabled: boolean) => {
    if (typeof enabled !== 'boolean') throw new Error('Invalid enable flag')
    if (port !== 0 && port !== 1) throw new Error('Invalid UART')
    if (!enabled) ethernetTransport.cancel(port)
    log('info', 'network.request', { port, enabled })
    return serializeNetwork(() => networkService.enable(port, enabled))
  })
  ipcMain.handle('kbm-network:forward', (_event, rules: ForwardRule[]) =>
    serializeNetwork(() => networkService.startForwarding(rules)))
  ipcMain.handle('kbm-network:stop-forward', () => serializeNetwork(async () => networkService.stopForwarding()))
}

async function createWindow(): Promise<void> {
  const mainWindow = new BrowserWindow({
    width: 1440,
    height: 900,
    minWidth: 960,
    minHeight: 640,
    backgroundColor: '#070b12',
    autoHideMenuBar: true,
    useContentSize: true,
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      preload: path.join(import.meta.dirname, 'electron-preload.cjs')
    }
  })
  mainWindow.on('blur', () => { void miniKeyboard.deactivate() })
  mainWindow.on('closed', () => { void miniKeyboard.stop() })
  mainWindow.webContents.on('render-process-gone', () => { void miniKeyboard.stop() })
  mainWindow.webContents.on('render-process-gone', (_event, details) => log('error', 'renderer.exited', { reason: details.reason, code: details.exitCode }))
  mainWindow.webContents.on('did-fail-load', (_event, code, description) => log('error', 'renderer.load_failed', { code, description }))

  if (import.meta.env.QUASAR_DEV) {
    await mainWindow.loadURL(import.meta.env.QUASAR_APP_URL)
  } else {
    await mainWindow.loadFile('index.html')
  }
}

void app.whenReady().then(async () => {
  if (uiBenchmark) {
    registerQuasarRuntime()
    try {
      await runUiBenchmark(uiBenchmark, path.join(import.meta.dirname, 'electron-preload.cjs'), async window => {
        if (import.meta.env.QUASAR_DEV) await window.loadURL(import.meta.env.QUASAR_APP_URL)
        else await window.loadFile('index.html')
      })
    } catch (error) { console.error('UI_BENCHMARK_FAILED', error); app.exit(1) }
    return
  }
  // Explicit discovery smoke test: SYSTEM only, no window/capture/input/SD writes.
  if (process.argv.includes('--mini-inspect')) {
    try {
      await miniService.scan()
      await new Promise(resolve => setTimeout(resolve, 1200))
      const state = miniService.state()
      console.log('MINIKMB_INSPECT', JSON.stringify(state))
      await miniService.stop()
      app.exit(state.boards.some(b => b.channels.some(c => c.connected)) ? 0 : 1)
    } catch (e) { console.error('MINIKMB_INSPECT_FAILED', e); await miniService.stop(); app.exit(1) }
    return
  }
  // Packaged-runtime regression check: virtual NICs only, no master discovery,
  // no physical frames, no listeners and no application windows.
  if (process.argv.includes('--network-self-test')) {
    const streamingBefore = typeof WebAssembly.instantiateStreaming
    const probe = new NetworkService(async () => {})
    try {
      await probe.enable(0, true)
      await probe.enable(1, true)
      const enabled = probe.snapshot().enabled
      await probe.stop()
      console.log('NETWORK_SELF_TEST', JSON.stringify({ ok: enabled.every(Boolean),
        streamingBefore, streamingAfter: typeof WebAssembly.instantiateStreaming,
        enabled, stopped: probe.snapshot().enabled }))
      app.exit(enabled.every(Boolean) ? 0 : 1)
    } catch (error) {
      console.error('NETWORK_SELF_TEST_FAILED', error)
      app.exit(1)
    }
    return
  }
  registerQuasarRuntime()
  log('info', 'app.started', { version: app.getVersion(), electron: process.versions.electron,
    protocol: 3, build: 'minikmb-app-1.0.0-20261120' })
  Menu.setApplicationMenu(null)
  configureDeviceAccess()
  registerIpc()
  await createWindow()
  hidDiagnosticsTimer = setInterval(() => hidDiagnostics.tick(),1000)
  diagnosticsTimer = setInterval(() => {
    const disks = diskTransfer.snapshot(), network = networkService.snapshot()
    for (const port of [0, 1] as const) {
      const d = disks[port]
      log('info', 'diagnostics.summary', { port, diskSectors: d.sectors, diskErrors: d.errors,
        diskSourceMs: Math.round(d.sourceMs), diskTransferMs: Math.round(d.transferMs),
        networkOn: network.enabled[port], rxFrames: network.rxFrames[port], txFrames: network.txFrames[port],
        dropped: network.dropped[port], queued: network.queued[port], connections: network.connections })
    }
  }, 10000)

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) void createWindow()
  })
})

let quitFlushed = false
app.on('before-quit', event => {
  if (uiBenchmark) return // No production services or logger started.
  if (quitFlushed) return
  event.preventDefault()
  log('info', 'app.stopping')
  networkService.stopForwarding()
  if (serialScanTimer) clearInterval(serialScanTimer)
  if (diagnosticsTimer) clearInterval(diagnosticsTimer)
  if (hidDiagnosticsTimer) clearInterval(hidDiagnosticsTimer)
  void Promise.allSettled([miniKeyboard.stop(), specialInput.stop(), miniCapture.stop(), miniFeatures.stop()]).finally(async () => {
    await miniService.stop()
    if (masterPort?.isOpen) masterPort.close()
    await operationLogger.flush(); quitFlushed = true; app.quit()
  })
})

app.on('window-all-closed', () => {
  if (platform !== 'darwin') app.quit()
})
