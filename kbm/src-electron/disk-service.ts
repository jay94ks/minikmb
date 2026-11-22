import { FolderDisk } from './folder-disk'
import type { DiskReadRequest, DiskShareState, DiskSnapshotInfo } from '../src/shared/disk'
import { validateDiskToken } from '../src/shared/disk'
import type { NetworkPort } from '../src/shared/network'
export class DiskService {
  private disks: [FolderDisk | undefined, FolderDisk | undefined] = [undefined, undefined]
  private pending = [false, false]
  private epochs = [0, 0]
  revision(port: NetworkPort): number { this.check(port); return this.epochs[port]! }
  isCurrent(port: NetworkPort, generation: number): boolean { return this.disks[port]?.info.generation === generation }
  snapshot(): DiskShareState { return { snapshots: this.disks.map(d => d ? { ...d.info } : null) as DiskShareState['snapshots'] } }
  async prepare(port: NetworkPort, folder: string): Promise<DiskSnapshotInfo> {
    this.check(port)
    const epoch = ++this.epochs[port]!
    this.disks[port] = undefined
    const disk = await FolderDisk.create(folder)
    if (epoch !== this.epochs[port]) throw new Error('디스크 생성이 취소되었습니다.')
    this.disks[port] = disk; return { ...disk.info }
  }
  disable(port: NetworkPort): void { this.check(port); ++this.epochs[port]!; this.disks[port] = undefined }
  async read(port: NetworkPort, request: DiskReadRequest): Promise<Uint8Array> {
    this.check(port)
    validateDiskToken(request)
    const disk = this.disks[port]
    if (!disk || request.generation !== disk.info.generation) throw new Error('Expired disk snapshot')
    const sectors = request.sectors ?? 1
    if (request.lba + sectors > disk.info.sectors) throw new Error('Disk read outside snapshot')
    if (this.pending[port]) throw new Error('Disk request already in flight')
    this.pending[port] = true
    try {
      const bytes = await disk.readSectors(request.lba, sectors)
      if (this.disks[port] !== disk) throw new Error('Expired disk read')
      return bytes
    } finally { this.pending[port] = false }
  }
  private check(port: NetworkPort): void { if (port !== 0 && port !== 1) throw new Error('Invalid UART') }
}
