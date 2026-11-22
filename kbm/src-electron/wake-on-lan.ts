import dgram from 'node:dgram'
import os from 'node:os'
import { normalizeWakeMac, WolCode, type WolAdapter, type WolConfig, type WakeResult } from '../src/shared/wake'
import type { OperationLog } from '../src/shared/logging'

export function magicPacket(mac: string): Uint8Array {
  const bytes = normalizeWakeMac(mac).split(':').map(value => parseInt(value, 16))
  const packet = new Uint8Array(102).fill(255)
  for (let n = 0; n < 16; n++) packet.set(bytes, 6 + n * 6)
  return packet
}
export class WakeOnLan {
  constructor(private log: OperationLog = () => {}, private interfaces = os.networkInterfaces,
    private socket = () => dgram.createSocket('udp4')) {}
  adapters(): WolAdapter[] {
    return Object.entries(this.interfaces()).flatMap(([name, addresses]) => (addresses ?? []).flatMap(value => {
      if (value.internal || value.family !== 'IPv4') return []
      const ip = value.address.split('.').map(Number), mask = value.netmask.split('.').map(Number)
      const broadcast = ip.map((part, index) => (part | (~mask[index]! & 255))).join('.')
      return [{ name, address: value.address, broadcast }]
    }))
  }
  async send(port: number, config: WolConfig): Promise<WakeResult> {
    if ((port !== 0 && port !== 1) || !config || typeof config.enabled !== 'boolean') throw new Error('Invalid WOL request')
    if (!config.enabled) {
      this.log('info', 'wake.wol_result', { port, code: WolCode.IgnoredDisabled })
      return { code: WolCode.IgnoredDisabled }
    }
    if (typeof config.mac !== 'string' || typeof config.localAddress !== 'string') throw new Error('Invalid WOL settings')
    const packet = magicPacket(config.mac)
    const adapter = this.adapters().find(value => value.address === config.localAddress)
    if (!adapter) throw new Error('선택한 LAN IPv4 주소가 현재 PC에 없습니다. 어댑터를 다시 선택하세요.')
    // Explicit host NIC + directed broadcast, not the app's UART/NCM transport.
    // No DNS, arbitrary destination ports or automatically chosen default route.
    let socket: ReturnType<typeof dgram.createSocket> | undefined
    let result: WakeResult
    try {
      const active = socket = this.socket()
      await new Promise<void>((resolve, reject) => {
        let finished = false
        const timer = setTimeout(() => finish(new Error('WOL 송신 시간 초과')), 2000)
        const finish = (error?: Error | null): void => {
          if (finished) return
          finished = true; clearTimeout(timer); error ? reject(error) : resolve()
        }
        active.on('error', finish)
        try { active.bind(0, adapter.address, () => {
          if (finished) return
          try { active.setBroadcast(true); active.send(packet, 9, adapter.broadcast, finish) }
          catch (error) { finish(error as Error) }
        }) } catch (error) { finish(error as Error) }
      })
      result = { code: WolCode.SentUnconfirmed }
    } catch (error) {
      if (!['ENOTSUP', 'EOPNOTSUPP', 'EAFNOSUPPORT', 'ENOSYS'].includes((error as NodeJS.ErrnoException).code ?? '')) {
        this.log('error', 'wake.wol_failed', { port, error: error instanceof Error ? error.message : String(error) }); throw error
      }
      result = { code: WolCode.IgnoredUnsupported }
    } finally { try { socket?.close() } catch { /* Never bound or already closed. */ } }
    this.log('info', 'wake.wol_result', { port, code: result.code })
    return result
  }
}
