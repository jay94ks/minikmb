import { NetworkService } from './network-service'
import { MiniNetworkWire } from './minikmb-network'
import { SpecialInput, SystemControlRejected } from './special-input'
import { MiniStorage } from './minikmb-storage'
import type { MiniService } from './minikmb-service'
import type { MiniConnection } from './minikmb-connection'
import type { MiniToolsState, NetworkConfig, ForwardRule, KeyChordRequest, MiniStorageAction, MiniNetworkMode } from '../src/shared/mini-tools'
import type { PortScanRequest } from '../src/shared/network'
import { defaultNetworkConfig } from '../src/shared/network'
import { channelName, type MiniInventory } from '../src/shared/minikmb'
import type { OperationLog } from '../src/shared/logging'

interface Context {
  uid: string; client: MiniConnection; network: NetworkService; wire: MiniNetworkWire; input: SpecialInput
  storage: MiniStorage; config: NetworkConfig; rules: ForwardRule[]; binding: Promise<void>; unsubscribe: () => void
  serial: Uint8Array; serialAt: number; serialSize: number; serialBytes: number; serialConsumed: bigint; serialLimit: bigint; creditBusy: boolean
  offline: boolean
}
export class MiniFeatures {
  private contexts = new Map<string, Context>()
  async rename(uid: string, name: string): Promise<void> {
    const requested = channelName(name)
    this.log('info', 'channel.name_save_started', { uid })
    try {
      await (await this.context(uid)).storage.action({ kind: 'channel-name-set', name: requested })
      await this.management.refreshName(uid)
      const saved = this.management.channel(uid)
      if (saved.nameError) throw new Error(saved.nameError)
      if (saved.name !== requested) throw new Error('저장 후 읽은 채널 이름이 다릅니다. 다시 시도하세요.')
      this.log('info', 'channel.name_saved', { uid })
    } catch (e) {
      this.log('error', 'channel.name_save_failed', { uid })
      throw e
    }
  }
  async prepareInput(uid: string): Promise<void> { const c = this.contexts.get(uid); if (c?.input.blocked(0)) await c.input.cancel(0) }
  async deactivate(uid: string): Promise<void> { const c = this.contexts.get(uid); if (c) await c.input.cancel(0) }
  constructor(private management: MiniService, private log: OperationLog,
    private chooseFile: (type: 'optical' | 'disk' | 'folder') => Promise<string | undefined>) {}
  private async context(uid: string): Promise<Context> {
    const channel = this.management.channel(uid), client = this.management.connection(uid)
    let ctx = this.contexts.get(uid)
    if (!ctx) {
      if (this.contexts.size >= 64) throw new Error('관리할 수 있는 채널 한도64개를 초과했습니다.')
      const scopedLog: OperationLog = (level, event, fields) => this.log(level, event, { ...fields, uid })
      const network = new NetworkService((_port, frame) => ctx!.wire.send(frame), scopedLog)
      const input = new SpecialInput(async (_port, opcode, payload) => {
        const current = this.management.connection(uid)
        let op: number, body: Uint8Array
        if (opcode === 0x7f) { op = 0x107; body = new Uint8Array() }
        else if (opcode === 0x10 || opcode === 0x11) { op = opcode === 0x10 ? 0x101 : 0x102; body = Uint8Array.of(payload[0]!, 0) }
        else {
          throw new SystemControlRejected('unsupported', '현재 STM32 프로토콜에는 System Control 명령이 없습니다.')
        }
        await current.hid(op, body, () => ctx?.client === current)
      }, () => {
        try { const c = this.management.channel(uid); return c.connected && Boolean(c.features & 1) && [1, 2].includes(c.targetState) }
        catch { return false }
      }, scopedLog)
      ctx = { uid, client, network, input, wire: undefined!, storage: undefined!, config: { ...defaultNetworkConfig }, rules: [], binding: Promise.resolve(), unsubscribe: () => {},
        serial: new Uint8Array(65536), serialAt: 0, serialSize: 0, serialBytes: 0, serialConsumed: 0n, serialLimit: 32768n, creditBusy: false, offline: false }
      ctx.storage = new MiniStorage(() => this.management.connection(uid), this.chooseFile, async () => {
        await this.management.releaseInput(); await ctx!.network.stop(); await ctx!.wire.enable(false)
      })
      ctx.binding = this.bind(ctx, client); this.contexts.set(uid, ctx)
    } else if (ctx.client !== client) {
      const previous = ctx.wire; ctx.unsubscribe(); previous.dispose(); ctx.input.restarted(0)
      ctx.client = client
      const target = ctx
      ctx.binding = ctx.binding.catch(() => {}).then(async () => { await target.network.stop(); await this.bind(target, client) })
    }
    await ctx.binding
    if (!(channel.features & 2)) throw new Error('현재 펌웨어에 타깃 기능이 없습니다.')
    return ctx
  }
  private bind(ctx: Context, client: MiniConnection): Promise<void> {
    ctx.offline = false
    ctx.serialAt = ctx.serialSize = ctx.serialBytes = 0; ctx.serialConsumed = 0n; ctx.serialLimit = 32768n; ctx.creditBusy = false
    ctx.wire = new MiniNetworkWire(client, frame => ctx.network.receive(0, Array.from(frame)))
    ctx.unsubscribe = client.subscribe(frame => {
      if (frame.kind === 4 && frame.channel === 2 && frame.opcode === 0x280 && frame.payload.length <= 256) {
        for (const b of frame.payload) { ctx.serial[ctx.serialAt] = b; ctx.serialAt = (ctx.serialAt + 1) % ctx.serial.length; ctx.serialSize = Math.min(ctx.serialSize + 1, ctx.serial.length) }
        ctx.serialBytes += frame.payload.length; ctx.serialConsumed += BigInt(frame.payload.length)
        if (ctx.serialConsumed >= 16384n) void this.serialCredit(ctx).catch(() => {})
      }
    })
    // Each app/session starts with the frame bridge OFF, regardless of a prior tool.
    // The raw terminal stays off until it is opened by the user.
    return client.features & 0x20 ? client.command(4, 0x401, Uint8Array.of(0)).then(() => {}) : Promise.resolve()
  }
  private async serialCredit(ctx: Context): Promise<void> {
    if (ctx.creditBusy) return
    const client = ctx.client
    ctx.creditBusy = true
    try {
      do {
        ctx.serialLimit += ctx.serialConsumed; ctx.serialConsumed = 0n
        const b = new Uint8Array(12); b[0] = 2; new DataView(b.buffer).setBigUint64(4, ctx.serialLimit, true)
        await client.command(0, 6, b)
      } while (ctx.client === client && ctx.serialConsumed >= 16384n)
    } finally { if (ctx.client === client) ctx.creditBusy = false }
  }
  async state(uid: string): Promise<MiniToolsState> {
    const c = await this.context(uid)
    const [status, serial, serialStats, net] = await Promise.all([c.client.command(0, 4), c.client.command(2, 0x202), c.client.command(2, 0x203), c.client.command(4, 0x403)])
    if (status.length !== 64 || serial.length !== 16 || serialStats.length !== 8 || net.length !== 16) throw new Error('진단 응답 길이 오류')
    const s = new DataView(status.buffer), t = new DataView(serial.buffer), counters = new DataView(serialStats.buffer)
    const received = new Uint8Array(c.serialSize)
    for (let i = 0; i < received.length; i++) received[i] = c.serial[(c.serialAt - c.serialSize + c.serial.length + i) % c.serial.length]!
    return { uid, features: c.client.features, input: c.input.state()[0], config: { ...c.config }, rules: c.rules.map(r => ({ ...r })),
      serial: { open: Boolean(serial[0]), baud: t.getUint32(4, true), dataBits: serial[3]!, parity: serial[2]!, stopBits: serial[1]!,
        txQueued: t.getUint32(8, true), rxQueued: t.getUint32(12, true), txDiscarded: counters.getUint32(0, true), rxDiscarded: counters.getUint32(4, true), received: Array.from(received), bytesReceived: c.serialBytes },
      network: c.network.snapshot(), networkUp: Boolean(net[13]), networkDropped: c.wire.dropped,
      networkMode: c.network.snapshot().enabled[0] ? c.network.isSharing(0) ? 'host' : 'emulation' : 'off', scan: c.network.scanState(0),
      diagnostics: { uptime: Number(s.getBigUint64(0, true)), crcErrors: s.getUint32(32, true), framingErrors: s.getUint32(36, true), eventDropped: s.getUint32(60, true), logDropped: s.getUint32(28, true), storageState: status[13]! }, storage: c.storage.state() }
  }
  async terminal(uid: string): Promise<void> { await this.serialCredit(await this.context(uid)) }
  async serialWrite(uid: string, bytes: unknown): Promise<void> {
    if (!Array.isArray(bytes) || bytes.length < 1 || bytes.length > 65536 || bytes.some(b => !Number.isInteger(b) || b < 0 || b > 255)) throw new Error('시리얼 송신 길이/형식 오류')
    const c = await this.context(uid)
    for (let offset = 0; offset < bytes.length; offset += 256) {
      const body = Uint8Array.from(bytes.slice(offset, offset + 256)), result = await c.client.command(2, 0x201, body)
      if (result.length !== 2 || new DataView(result.buffer).getUint16(0, true) !== body.length) throw new Error('시리얼 수락 길이 불일치')
    }
  }
  async chord(uid: string, request: KeyChordRequest): Promise<void> { const c = await this.context(uid); await this.management.releaseInput(); await this.prepareInput(uid); await c.input.chord(0, request) }
  async system(uid: string, usage: number, confirmed: boolean): Promise<void> { const c = await this.context(uid); await this.management.releaseInput(); await this.prepareInput(uid); await c.input.system(0, usage, confirmed) }
  async release(uid: string): Promise<void> { const c = await this.context(uid); await this.management.releaseInput(); await this.prepareInput(uid); await c.input.release(0) }
  async configure(uid: string, config: NetworkConfig): Promise<void> { const c = await this.context(uid); await c.network.configure(config); c.config = { ...config } }
  async scan(uid: string, request: PortScanRequest | null): Promise<void> { (await this.context(uid)).network.scan(0, request) }
  async network(uid: string, mode: MiniNetworkMode): Promise<void> {
    if (!['off', 'emulation', 'host'].includes(mode)) throw new Error('네트워크 모드 오류')
    const c = await this.context(uid)
    await c.wire.enable(false); await c.network.stop()
    if (mode !== 'off') {
      await c.network.enable(0, true, mode === 'host')
      try { await c.wire.enable(true) } catch (e) { await c.network.stop(); throw e }
    }
  }
  async forward(uid: string, rules: ForwardRule[]): Promise<void> {
    if (!Array.isArray(rules) || rules.length > 32 || rules.some(r => r.port !== 0)) throw new Error('선택 채널의 포워딩 규칙만 허용합니다.')
    const c = await this.context(uid); await c.network.startForwarding(rules); c.rules = rules.map(r => ({ ...r }))
  }
  async stopForward(uid: string): Promise<void> { (await this.context(uid)).network.stopForwarding() }
  async storage(uid: string, action: MiniStorageAction): Promise<void> { await (await this.context(uid)).storage.action(action) }
  observe(inventory: MiniInventory): void {
    for (const ctx of this.contexts.values()) if (!ctx.offline && !inventory.boards.some(b => b.channels.some(c => c.uid === ctx.uid && c.connected))) {
      ctx.offline = true; ctx.storage.cancelLocal(); ctx.input.restarted(0); ctx.wire.dispose(); ctx.network.stopForwarding(); void ctx.network.stop().catch(() => {})
    }
  }
  async stop(): Promise<void> {
    await Promise.allSettled([...this.contexts.values()].map(ctx => ctx.storage.stop()))
    for (const ctx of this.contexts.values()) { ctx.unsubscribe(); ctx.wire.dispose() }
    await Promise.allSettled([...this.contexts.values()].map(async ctx => { await ctx.network.stop(); await ctx.input.stop() }))
  }
}
