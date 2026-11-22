import { Op } from '../src/services/protocol'
import { crc32 } from '../src/services/ethernet'
import type { DiskReadRequest, DiskTransferMetrics } from '../src/shared/disk'
import type { DiskService } from './disk-service'
import type { RoutedTransport } from './routed-transport'
import type { OperationLog } from '../src/shared/logging'
type Port = 0 | 1
const initial = (): DiskTransferMetrics => ({ sectors: 0, sourceMs: 0, transferMs: 0, totalMs: 0, duplicates: 0, errors: 0 })
// Sector serving stays in Electron main: zero renderer IPC per fragment/ACK.
export class DiskTransfer {
  private busy = [false, false]
  private revisions = [-1, -1]
  private metrics: [DiskTransferMetrics, DiskTransferMetrics] = [initial(), initial()]
  constructor(private disks: DiskService, private transport: RoutedTransport,
    private now: () => number = () => performance.now(), private log: OperationLog = () => {}) {}
  snapshot(): [DiskTransferMetrics, DiskTransferMetrics] {
    return this.metrics.map(value => ({ ...value })) as [DiskTransferMetrics, DiskTransferMetrics]
  }
  private refresh(port: Port): void {
    const revision = this.disks.revision(port)
    if (this.revisions[port] !== revision) { this.revisions[port] = revision; this.metrics[port] = initial() }
  }
  async serve(port: Port, token: DiskReadRequest): Promise<void> {
    this.refresh(port)
    if (!token.generation || !token.id || !this.disks.isCurrent(port, token.generation)) return
    if (this.busy[port]) { ++this.metrics[port].duplicates; return } // F8 will retry at500ms.
    this.busy[port] = true
    this.log('debug', 'disk.read', { port, generation: token.generation, id: token.id, lba: token.lba, sectors: token.sectors ?? 1 })
    const revision = this.disks.revision(port), stats = this.metrics[port], started = this.now()
    const current = (): boolean => revision === this.disks.revision(port) && this.disks.isCurrent(port, token.generation)
    try {
      const bytes = await this.disks.read(port, token), readAt = this.now()
      if (!current()) return
      if(bytes.length!==(token.sectors ?? 1)*512 || bytes.length>4096) throw new Error('Invalid disk read block')
      stats.sourceMs += readAt - started
      const crc = crc32(bytes)
      for (let offset = 0; offset < bytes.length; offset += 4096) {
        if (!current()) return
        const payload = new Uint8Array(18+Math.min(4096,bytes.length-offset)), view = new DataView(payload.buffer)
        view.setUint32(0, token.generation, true); view.setUint32(4, token.id, true); view.setUint32(8, token.lba, true)
        view.setUint16(12, offset, true); view.setUint32(14, crc, true); payload.set(bytes.subarray(offset, offset + 4096), 18)
        const response = await this.transport.request({ op: Op.Route, payload: Uint8Array.of(port, 0, 0, Op.DiskData, ...payload) }, true, current)
        if (response.payload[3] !== 0 || response.payload[5] !== 0) throw new Error('Disk fragment rejected')
      }
      if (current()) {
        stats.sectors+=token.sectors ?? 1; stats.transferMs += this.now() - readAt; stats.totalMs += this.now() - started
      }
    } catch (error) {
      if (current()) {
        ++stats.errors
        this.log('error', 'disk.read_failed', { port, generation: token.generation, id: token.id,
          lba: token.lba, sectors: token.sectors ?? 1, error: error instanceof Error ? error.message : String(error) })
        const p = new Uint8Array(12), view = new DataView(p.buffer)
        view.setUint32(0, token.generation, true); view.setUint32(4, token.id, true); view.setUint32(8, token.lba, true)
        await this.transport.request({ op: Op.Route, payload: Uint8Array.of(port, 0, 0, Op.DiskFail, ...p) }, false, current).catch(() => {})
      }
    } finally { this.busy[port] = false }
  }
}
