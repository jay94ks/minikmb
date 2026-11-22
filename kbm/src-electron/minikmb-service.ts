import { Worker } from 'node:worker_threads'
import { SerialPort } from 'serialport'
import { MiniConnection } from './minikmb-connection'
import { groupMiniBoards } from './minikmb-topology'
import { miniTopologyScript } from './minikmb-windows'
export { miniTopologyScript } from './minikmb-windows'
import { channelName, inputPacket, validUid, type MiniChannel, type MiniInput, type MiniInventory, type MiniUsbNode } from '../src/shared/minikmb'
export async function miniUsbTopology(): Promise<MiniUsbNode[]> {
  if (process.platform !== 'win32') return []
  const encoded = Buffer.from(miniTopologyScript, 'utf16le').toString('base64')
  // Windows process creation/PnP startup can stall the calling thread. Keep it
  // away from USB response handling and the MCU's 2-second session heartbeat.
  const stdout = await new Promise<string>((resolve, reject) => {
    const worker = new Worker(`
      const { parentPort, workerData } = require('node:worker_threads');
      require('node:child_process').execFile('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', workerData],
        { windowsHide: true, timeout: 15000, maxBuffer: 2 * 1024 * 1024 }, (error, stdout) => {
          parentPort.postMessage(error ? { error: error.message } : { stdout });
        });
    `, { eval: true, workerData: encoded })
    let settled = false
    const timer = setTimeout(() => { if (!settled) { settled = true; reject(new Error('USB 연결 구조 조회 시간 초과')); void worker.terminate() } }, 18000)
    worker.once('message', (message: { stdout?: string; error?: string }) => {
      if (settled) return
      settled = true; clearTimeout(timer)
      if (typeof message.stdout === 'string') resolve(message.stdout)
      else reject(new Error(message.error ?? 'USB 연결 구조 조회 실패'))
    })
    worker.once('error', error => { if (!settled) { settled = true; clearTimeout(timer); reject(error) } })
    worker.once('exit', code => { if (!settled) { settled = true; clearTimeout(timer); reject(new Error(`USB 조회 작업 종료 (${code})`)) } })
    worker.unref()
  })
  const parsed: unknown = JSON.parse(stdout.trim() || '[]')
  const records = Array.isArray(parsed) ? parsed : [parsed]
  return records.filter((n): n is MiniUsbNode => Boolean(n && typeof n === 'object' && typeof n.id === 'string' && typeof n.parent === 'string' &&
    typeof n.container === 'string' && Array.isArray(n.location) && n.location.every((v: unknown) => typeof v === 'string') && Number.isInteger(n.address)))
}
export class MiniService {
  private connections = new Map<string, { client: MiniConnection; state: MiniChannel; pulse: boolean; fingerprint: string; shutdown: Promise<void> }>()
  private scanning: Promise<MiniInventory> | undefined
  private nodes: MiniUsbNode[] = []
  private topologyAt = 0
  private scanTimer: ReturnType<typeof setInterval> | undefined
  private pulseTimer: ReturnType<typeof setInterval> | undefined
  private stopped = false
  private inputUid = ''
  private epoch = 0
  private selectionRequest = 0
  private snapshot: MiniInventory = { boards: [], error: '', scannedAt: 0 }
  private nameReadAt = new WeakMap<MiniConnection, number>()
  async refreshName(uid: string): Promise<void> {
    const entry = this.connections.get(uid)
    if (!entry?.state.connected) throw new Error('채널 MCU가 연결되지 않았습니다.')
    this.nameReadAt.set(entry.client, Date.now())
    try {
      const reply = await entry.client.command(3, 0x31e), length = new DataView(reply.buffer, reply.byteOffset, reply.byteLength).getUint16(0, true)
      if (reply.length !== 4 + length || length > 64 || reply[2] || reply[3]) throw new Error('채널 이름 응답 형식 오류')
      entry.state.name = channelName(new TextDecoder('utf-8', { fatal: true }).decode(reply.subarray(4)))
      entry.state.nameSupported = true; entry.state.nameError = ''
    } catch (e) {
      const status = (e as { status?: number }).status
      if (status === 1) { entry.state.nameSupported = false; entry.state.name = ''; entry.state.nameError = '이름 저장을 지원하는 MCU 펌웨어가 필요합니다.' }
      else if (status === 9 || status === 17) { entry.state.nameSupported = true; entry.state.name = ''; entry.state.nameError = status === 9 ? 'SD 카드가 없습니다.' : '' }
      else { entry.state.name = ''; entry.state.nameError = status === 3 ? 'SD 카드의 MCU UID를 확인하세요.' : String(e) }
    }
    this.publish()
  }
  constructor(private readonly notify: (snapshot: MiniInventory) => void) {}
  state(): MiniInventory { return this.snapshot }
  channel(uid: unknown): MiniChannel {
    if (!validUid(uid)) throw new Error('MCU UID 오류')
    const channel = this.snapshot.boards.flatMap(b => b.channels).find(c => c.uid === uid)
    if (!channel?.connected) throw new Error('채널 MCU가 연결되지 않았습니다.')
    return channel
  }
  connection(uid: unknown): MiniConnection {
    this.channel(uid)
    return this.connections.get(uid as string)!.client
  }
  private publish(error = ''): MiniInventory {
    this.snapshot = { boards: groupMiniBoards(this.nodes, [...this.connections.values()].filter(c => validUid(c.state.uid)).map(c => ({ ...c.state }))), error, scannedAt: Date.now() }
    this.notify(this.snapshot); return this.snapshot
  }
  async scan(): Promise<MiniInventory> {
    if (this.scanning) return this.scanning
    if (this.stopped) throw new Error('앱을 종료하는 중입니다.')
    if (!this.scanTimer) {
      this.scanTimer = setInterval(() => void this.scan().catch(() => {}), 5000)
      this.pulseTimer = setInterval(() => void this.pulse(), 500)
    }
    this.scanning = this.discover().catch(e => this.publish(String(e))).finally(() => { this.scanning = undefined })
    return this.scanning
  }
  private async discover(): Promise<MiniInventory> {
    const failures: string[] = []
    const ports = (await SerialPort.list()).filter(p => p.vendorId?.toLowerCase() === 'cafe' && /^400[124]$/i.test(p.productId ?? ''))
    if (this.stopped) return this.snapshot
    for (const [uid, entry] of this.connections) {
      if (!ports.some(p => p.path === entry.state.port && `${p.pnpId ?? ''}|${p.serialNumber ?? ''}` === entry.fingerprint)) {
        entry.client.close('MCU 분리'); this.connections.delete(uid)
      }
    }
    for (const port of ports) {
      // VID/PID finds the management transport; only HELLO/INFO establishes UID.
      const uid = ''
      if ([...this.connections.values()].some(c => c.state.port === port.path && c.state.connected)) continue
      const pendingKey = `pending:${port.path}`
      const state: MiniChannel = { uid, boardId: '', port: port.path, usbPort: 0, connected: false, features: 0, targetState: 0, error: '', slot: null, captureId: null, pairingError: '' }
      const native = new SerialPort({ path: port.path, baudRate: 115200, autoOpen: false })
      let opening = true, finishClose = () => {}
      const shutdown = new Promise<void>(resolve => { finishClose = resolve })
      const client = new MiniConnection(uid, {
        write: bytes => new Promise<void>((resolve, reject) => native.write(Buffer.from(bytes), e => e ? reject(e) : resolve())),
        close: () => { if (native.isOpen) native.close(() => finishClose()); else if (!opening) finishClose() }
      }, message => { state.connected = false; state.error = message; if (this.inputUid === state.uid) { this.inputUid = ''; this.epoch++ }; this.publish() })
      const entry = { client, state, pulse: false, fingerprint: `${port.pnpId ?? ''}|${port.serialNumber ?? ''}`, shutdown }
      this.connections.set(pendingKey, entry)
      native.on('data', (bytes: Buffer) => client.feed(bytes))
      native.on('error', () => client.close('관리 포트를 열 수 없습니다. 다른 진단 도구가 사용 중인지 확인하세요.'))
      native.on('close', () => { finishClose(); client.close('관리 포트 종료') })
      try {
        await new Promise<void>((resolve, reject) => native.open(e => { opening = false; if (e) { finishClose(); reject(e) } else resolve() }))
        if (this.stopped) { if (native.isOpen) native.close(() => finishClose()); client.close(); break }
        await client.hello()
        const duplicate = this.connections.get(client.uid)
        if (duplicate && duplicate !== entry && duplicate.state.port !== port.path) {
          duplicate.client.close('중복 MCU UID'); throw new Error('중복 MCU UID를 가진 장치를 구분할 수 없습니다.')
        }
        this.connections.delete(pendingKey); state.uid = client.uid
        this.connections.set(client.uid, entry); state.features = client.features; state.connected = true
        void this.pulse() // Do not wait for the next timer tick after HELLO/INFO.
      } catch (e) { const message = e instanceof Error ? e.message : String(e); failures.push(`${port.path}: ${message}`); client.close(message) }
    }
    if (Date.now() - this.topologyAt > 10000) {
      try { this.nodes = await miniUsbTopology(); this.topologyAt = Date.now() }
      catch { return this.publish('USB 연결 구조 조회 실패 · 자동 영상 연결을 중지했습니다. 다시 검색하세요.') }
    }
    return this.publish(failures.join(' · '))
  }
  private async pulse(): Promise<void> {
    for (const entry of this.connections.values()) {
      if (!entry.state.connected || entry.pulse) continue
      entry.pulse = true
      void (async () => {
        try {
          const cookie = new Uint8Array(8)
          const ping = await entry.client.command(0, 2, cookie)
          if (ping.length < 8 || ping.slice(0, 8).some(Boolean)) throw new Error('PING 응답 오류')
          const result = await entry.client.command(0, 4)
          if (result.length !== 64) throw new Error('STATUS 길이 오류')
          entry.state.targetState = result[12]!
          if (entry.state.nameSupported !== false && Date.now() - (this.nameReadAt.get(entry.client) ?? 0) >= 3000) await this.refreshName(entry.state.uid)
          this.publish()
        } catch { entry.client.close('상태 조회 실패') }
        finally { entry.pulse = false }
      })()
    }
  }
  async clearInput(): Promise<void> {
    ++this.selectionRequest
    const uid = this.inputUid; this.inputUid = ''; this.epoch++
    await this.releaseUid(uid)
  }
  async releaseInput(): Promise<void> {
    this.epoch++
    await this.releaseUid(this.inputUid)
  }
  isSelected(uid: string): boolean { return this.inputUid === uid }
  async releaseInputFor(uid: string): Promise<void> {
    if (!validUid(uid)) return
    if (this.inputUid === uid) this.epoch++
    await this.releaseUid(uid)
  }
  private async releaseUid(uid: string): Promise<void> {
    if (!uid) return
    const c = this.connections.get(uid)
    if (c?.state.connected && c.state.features & 1) {
      try { await c.client.hid(0x107, new Uint8Array(), () => true) }
      catch { c.client.close('키 해제 완료 미확인 · 세션을 종료했습니다.') }
    }
  }
  async selectInput(uid: unknown): Promise<void> {
    const request = ++this.selectionRequest
    const previous = this.inputUid; this.inputUid = ''; this.epoch++
    await this.releaseUid(previous)
    if (request !== this.selectionRequest || this.stopped) throw new Error('이전 채널 선택 취소')
    if (!validUid(uid)) throw new Error('MCU UID 오류')
    const c = this.connections.get(uid)
    if (!c?.state.connected) throw new Error('MCU 연결 종료')
    this.inputUid = uid
  }
  async input(uid: unknown, input: MiniInput): Promise<void> {
    if (!validUid(uid) || this.inputUid !== uid) throw new Error('현재 제어 중인 채널이 아닙니다.')
    const c = this.connections.get(uid), epoch = this.epoch
    if (!c?.state.connected || !(c.state.features & 1) || ![1, 2].includes(c.state.targetState)) throw new Error('HID 지원 펌웨어와 활성 타깃 연결이 필요합니다.')
    const packet = inputPacket(input)
    try { await c.client.hid(packet.opcode, packet.payload, () => this.epoch === epoch && this.inputUid === uid) }
    catch (e) { if (this.epoch === epoch) await this.releaseInput(); throw e }
  }
  async stop(): Promise<void> {
    this.stopped = true
    if (this.scanTimer) clearInterval(this.scanTimer)
    if (this.pulseTimer) clearInterval(this.pulseTimer)
    await this.clearInput()
    for (const c of this.connections.values()) c.client.close('앱 연결 종료')
    // Wait for overlapped native serial IO disposal before Electron app.exit/quit.
    await Promise.allSettled([...this.connections.values()].map(c => c.shutdown))
    this.connections.clear()
  }
}
