import fs from 'node:fs/promises'
import path from 'node:path'
import { randomBytes } from 'node:crypto'
import type { BigIntStats } from 'node:fs'
import type { DiskSnapshotInfo } from '../src/shared/disk'

const SECTOR = 512, SPC = 8, CLUSTER = SECTOR * SPC, RESERVED = 32
const MAX_ENTRIES = 10000, MAX_METADATA = 8 * 1024 * 1024, MAX_SECTORS = 67108864
interface Entry {
  name: string; filename: string; directory: boolean; children: Entry[]; parent?: Entry
  stat: BigIntStats; first: number; clusters: number; content?: Uint8Array
}
function contained(root: string, target: string): boolean {
  const relative = path.relative(root, target)
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative))
}
function unchanged(a: BigIntStats, b: BigIntStats): boolean {
  return a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeNs === b.mtimeNs && a.ctimeNs === b.ctimeNs
}
function shortName(index: number, name: string, directory: boolean): Uint8Array {
  const stem = `F${index.toString(36).toUpperCase().padStart(7, '0')}`
  const extension = directory ? '' : path.extname(name).slice(1).toUpperCase().replace(/[^A-Z0-9]/g, '').slice(0, 3)
  return new TextEncoder().encode(stem + extension.padEnd(3, ' '))
}
function shortChecksum(name: Uint8Array): number {
  let sum = 0
  for (const byte of name) sum = (((sum & 1) << 7) + (sum >> 1) + byte) & 255
  return sum
}
function entriesFor(name: string, alias: Uint8Array, cluster: number, size: number, directory: boolean): Uint8Array[] {
  const result: Uint8Array[] = [], length = Math.ceil(name.length / 13)
  const positions = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
  for (let ordinal = length; ordinal > 0; --ordinal) {
    const entry = new Uint8Array(32), view = new DataView(entry.buffer)
    entry[0] = ordinal | (ordinal === length ? 0x40 : 0); entry[11] = 0x0f; entry[13] = shortChecksum(alias)
    for (let n = 0; n < 13; ++n) {
      const at = (ordinal - 1) * 13 + n
      view.setUint16(positions[n]!, at < name.length ? name.charCodeAt(at) : at === name.length ? 0 : 0xffff, true)
    }
    result.push(entry)
  }
  const entry = new Uint8Array(32), view = new DataView(entry.buffer)
  entry.set(alias); entry[11] = directory ? 0x11 : 0x21
  // A valid deterministic FAT timestamp (1980-01-01); content is a read-only view.
  view.setUint16(16, 0x21, true); view.setUint16(18, 0x21, true); view.setUint16(24, 0x21, true)
  view.setUint16(20, cluster >>> 16, true); view.setUint16(26, cluster & 0xffff, true)
  view.setUint32(28, size, true); result.push(entry)
  return result
}

/** Sparse, read-only FAT32 superfloppy. Stores metadata, never a full disk image.
 * Folder manifest is frozen; sector reads reject files changed/replaced since it
 * was created. Symlinks/junctions are not followed. No source file is written.
 */
export class FolderDisk {
  readonly info: DiskSnapshotInfo
  // All allocated clusters form a prefix. The remaining image is zero-filled.
  get populatedSectors(): number { return RESERVED + 2 * this.fatSectors + this.used * SPC }
  private metadataCache = new Map<number, Uint8Array>() // bounded256KiB, immutable manifest only
  private constructor(private root: string, private extents: Entry[], private fatSectors: number,
    private clusters: number, private used: number, info: DiskSnapshotInfo) { this.info = info }

  static async create(folder: string): Promise<FolderDisk> {
    const root = await fs.realpath(folder), all: Entry[] = []
    let files = 0, bytes = 0, metadata = 0
    async function walk(filename: string, name: string, parent?: Entry, depth = 0): Promise<Entry> {
      if (depth > 32 || all.length >= MAX_ENTRIES) throw new Error('공유는 최대 10,000개 항목 / 깊이 32입니다.')
      const stat = await fs.lstat(filename, { bigint: true })
      if (stat.isSymbolicLink() || (!stat.isDirectory() && !stat.isFile())) throw new Error('심볼릭 링크/특수 파일은 공유할 수 없습니다.')
      if (!contained(root, await fs.realpath(filename))) throw new Error('공유 폴더 밖의 경로입니다.')
      if (name.length > 255 || /[\x00-\x1f\\/:*?"<>|]/.test(name) || /[. ]$/.test(name)) throw new Error(`USB 디스크에 표시할 수 없는 이름: ${name}`)
      if (stat.size > 0xffffffffn && stat.isFile()) throw new Error(`FAT32 파일은 4 GiB 미만이어야 합니다: ${name}`)
      const entry: Entry = { name, filename, directory: stat.isDirectory(), children: [], stat, first: 0, clusters: 0 }
      if (parent) entry.parent = parent
      all.push(entry)
      if (entry.directory) {
        const children = []
        const listing = await fs.opendir(filename)
        for await (const child of listing) {
          if (child.isSymbolicLink() || (!child.isDirectory() && !child.isFile())) continue
          if (children.length + all.length >= MAX_ENTRIES) throw new Error('폴더 항목 한도 10,000개를 초과했습니다.')
          children.push(child)
        }
        children.sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)
        for (const child of children) {
          if (child.isSymbolicLink() || (!child.isDirectory() && !child.isFile())) continue
          entry.children.push(await walk(path.join(filename, child.name), child.name, entry, depth + 1))
        }
        const slots = (parent ? 2 : 1) + entry.children.reduce((n, child) => n + Math.ceil(child.name.length / 13) + 1, 0) + 1
        entry.clusters = Math.max(1, Math.ceil(slots * 32 / CLUSTER))
        metadata += entry.clusters * CLUSTER
        if (metadata > MAX_METADATA) throw new Error('디렉터리 메타데이터 한도 8 MiB를 초과했습니다.')
      } else { ++files; bytes += Number(stat.size); entry.clusters = Math.ceil(Number(stat.size) / CLUSTER) }
      return entry
    }
    // Root display name is not stored as a VFAT filename.
    const tree = await walk(root, 'ROOT')
    if (!tree.directory) throw new Error('폴더를 선택하세요.')
    let next = 2
    for (const entry of all) if (entry.clusters) { entry.first = next; next += entry.clusters }
    // Above FAT32's65525-cluster threshold without doubling the metadata that
    // Windows repeatedly scans over a slow UART when mounting a small share.
    const used = next - 2, clusters = Math.max(65536, used + 1024), fatSectors = Math.ceil((clusters + 2) * 4 / SECTOR)
    const sectors = RESERVED + 2 * fatSectors + clusters * SPC
    if (sectors > MAX_SECTORS) throw new Error('가상 디스크는 최대 32 GiB입니다.')
    let aliasIndex = 0
    for (const dir of all.filter(e => e.directory)) {
      const content = new Uint8Array(dir.clusters * CLUSTER)
      let at = 0
      const append = (e: Uint8Array): void => { content.set(e, at); at += 32 }
      if (dir.parent) {
        const dot = (name: string, cluster: number): Uint8Array => {
          const e = new Uint8Array(32), v = new DataView(e.buffer)
          e.set(new TextEncoder().encode(name.padEnd(11, ' '))); e[11] = 0x10
          v.setUint16(20, cluster >>> 16, true); v.setUint16(26, cluster & 65535, true); return e
        }
        append(dot('.', dir.first)); append(dot('..', dir.parent === tree ? 0 : dir.parent.first))
      } else { const label = new Uint8Array(32); label.set(new TextEncoder().encode('KBM SHARE  ')); label[11] = 8; append(label) }
      for (const child of dir.children) for (const e of entriesFor(child.name, shortName(++aliasIndex, child.name, child.directory),
        child.first, child.directory ? 0 : Number(child.stat.size), child.directory)) append(e)
      dir.content = content
    }
    const generation = randomBytes(4).readUInt32LE() || 1
    return new FolderDisk(root, all.filter(e => e.clusters), fatSectors, clusters, used,
      { generation, folder: root, files, bytes, sectors })
  }
  private find(cluster: number): Entry | undefined {
    let lo = 0, hi = this.extents.length - 1
    while (lo <= hi) {
      const mid = (lo + hi) >>> 1, entry = this.extents[mid]!
      if (cluster < entry.first) hi = mid - 1
      else if (cluster >= entry.first + entry.clusters) lo = mid + 1
      else return entry
    }
    return undefined
  }
  private boot(): Uint8Array {
    const b = new Uint8Array(512), v = new DataView(b.buffer)
    b.set([0xeb, 0x58, 0x90]); b.set(new TextEncoder().encode('KBM FAT '), 3)
    v.setUint16(11, 512, true); b[13] = SPC; v.setUint16(14, RESERVED, true); b[16] = 2; b[21] = 0xf8
    v.setUint16(24, 63, true); v.setUint16(26, 255, true); v.setUint32(32, this.info.sectors, true)
    v.setUint32(36, this.fatSectors, true); v.setUint32(44, 2, true); v.setUint16(48, 1, true); v.setUint16(50, 6, true)
    b[64] = 0x80; b[66] = 0x29; v.setUint32(67, this.info.generation, true)
    b.set(new TextEncoder().encode('KBM SHARE  '), 71); b.set(new TextEncoder().encode('FAT32   '), 82)
    b[510] = 0x55; b[511] = 0xaa; return b
  }
  async readSector(lba: number): Promise<Uint8Array> {
    if (!Number.isInteger(lba) || lba < 0 || lba >= this.info.sectors) throw new Error('Invalid disk LBA')
    const cached = this.metadataCache.get(lba)
    if (cached) {
      this.metadataCache.delete(lba); this.metadataCache.set(lba, cached)
      return cached.slice()
    }
    const bytes = await this.generateSector(lba)
    const metadataEnd = RESERVED + 2 * this.fatSectors
    const entry = lba >= metadataEnd ? this.find(2 + Math.floor((lba - metadataEnd) / SPC)) : undefined
    // Never cache mutable source file sectors: every new source read must keep
    // realpath, file identity and before/after metadata validation intact.
    if (lba < metadataEnd || !entry || entry.directory) {
      if (this.metadataCache.size >= 512) this.metadataCache.delete(this.metadataCache.keys().next().value!)
      this.metadataCache.set(lba, bytes.slice())
    }
    return bytes
  }
  async readSectors(lba: number, sectors: number): Promise<Uint8Array> {
    if (!Number.isInteger(lba) || lba < 0 || !Number.isInteger(sectors) || sectors < 1 || sectors > 8 ||
        lba + sectors > this.info.sectors) throw new Error('Invalid disk read range')
    const bytes = new Uint8Array(sectors * SECTOR)
    const dataStart = RESERVED + 2 * this.fatSectors
    if (lba >= dataStart) {
      const first = this.find(2 + Math.floor((lba - dataStart) / SPC))
      const last = this.find(2 + Math.floor((lba + sectors - 1 - dataStart) / SPC))
      if (first && first === last && !first.directory) {
        const offset = (lba - dataStart - (first.first - 2) * SPC) * SECTOR
        // One open/read/stat pair per block, no mutable source-data cache.
        if (offset < Number(first.stat.size)) await this.readFileRange(first, offset, bytes)
        return bytes
      }
    }
    for (let n = 0; n < sectors; ++n) bytes.set(await this.readSector(lba + n), n * SECTOR)
    return bytes
  }
  private async generateSector(lba: number): Promise<Uint8Array> {
    if (lba === 0 || lba === 6) return this.boot()
    const b = new Uint8Array(512), v = new DataView(b.buffer)
    if (lba === 1 || lba === 7) {
      v.setUint32(0, 0x41615252, true); v.setUint32(484, 0x61417272, true)
      v.setUint32(488, this.clusters - this.used, true); v.setUint32(492, this.used + 2, true); v.setUint32(508, 0xaa550000, true)
      return b
    }
    if (lba < RESERVED) return b
    if (lba < RESERVED + 2 * this.fatSectors) {
      const first = ((lba - RESERVED) % this.fatSectors) * 128
      for (let i = 0; i < 128; ++i) {
        const cluster = first + i, entry = this.find(cluster)
        const value = cluster === 0 ? 0x0ffffff8 : cluster === 1 ? 0xffffffff :
          entry ? cluster === entry.first + entry.clusters - 1 ? 0x0fffffff : cluster + 1 : 0
        v.setUint32(i * 4, value, true)
      }
      return b
    }
    const dataSector = lba - RESERVED - 2 * this.fatSectors, cluster = 2 + Math.floor(dataSector / SPC)
    const entry = this.find(cluster)
    if (!entry) return b
    const offset = (cluster - entry.first) * CLUSTER + (dataSector % SPC) * SECTOR
    if (entry.content) { b.set(entry.content.subarray(offset, offset + 512)); return b }
    if (offset >= Number(entry.stat.size)) return b
    await this.readFileRange(entry, offset, b)
    return b
  }
  private async readFileRange(entry: Entry, offset: number, output: Uint8Array): Promise<void> {
    const resolved = await fs.realpath(entry.filename)
    if (!contained(this.root, resolved)) throw new Error('공유 폴더 밖으로 변경된 경로입니다.')
    const handle = await fs.open(resolved, 'r')
    try {
      if (!unchanged(entry.stat, await handle.stat({ bigint: true }))) throw new Error('공유 후 파일이 변경되었습니다. 공유를 새로고침하세요.')
      const size = Math.min(output.length, Number(entry.stat.size) - offset)
      const read = await handle.read(output, 0, size, offset)
      if (read.bytesRead !== size || !unchanged(entry.stat, await handle.stat({ bigint: true }))) throw new Error('읽는 동안 파일이 변경되었습니다.')
    } finally { await handle.close() }
  }
}
