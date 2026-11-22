export interface DiskSnapshotInfo {
  generation: number
  folder: string
  files: number
  bytes: number
  sectors: number
}
export interface DiskShareState { snapshots: [DiskSnapshotInfo | null, DiskSnapshotInfo | null] }
export interface DiskReadRequest { generation: number; id: number; lba: number; sectors?: number }
export interface DiskDeviceStatus { configured: boolean; ready: boolean }
export interface DiskTransferMetrics {
  sectors: number; sourceMs: number; transferMs: number; totalMs: number; duplicates: number; errors: number
}
export function isUint32(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 0xffffffff
}
export function validateDiskToken(request: DiskReadRequest): void {
  if (!request || !isUint32(request.generation) || !request.generation ||
      !isUint32(request.id) || !request.id || !isUint32(request.lba)) {
    throw new Error('Invalid disk read token')
  }
  if (!Number.isInteger(request.sectors ?? 1) || (request.sectors ?? 1) < 1 || (request.sectors ?? 1) > 8) throw new Error('Invalid disk sector count')
}
