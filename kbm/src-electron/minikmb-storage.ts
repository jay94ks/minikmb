import fs from 'node:fs/promises'
import path from 'node:path'
import { randomBytes } from 'node:crypto'
import { crc32 } from 'node:zlib'
import type { MiniConnection } from './minikmb-connection'
import type { MiniStorageAction, MiniStorageState } from '../src/shared/mini-tools'
import { FolderDisk } from './folder-disk'
import { makeFormattedImage } from './image-format'
import { channelName } from '../src/shared/minikmb'
import { TransferProgress } from './transfer-progress'

const pause = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))
const words = (...values: number[]) => { const b = new Uint8Array(values.length * 4), v = new DataView(b.buffer); values.forEach((n, i) => v.setUint32(i * 4, n, true)); return b }
const view = (b: Uint8Array) => new DataView(b.buffer, b.byteOffset, b.byteLength)
const number32 = (n: number) => Number.isInteger(n) && n >= 0 && n <= 0xffffffff
export class MiniStorage {
  private snapshot: MiniStorageState = { busy: false, action: '', done: 0, total: 0, error: '', image: null, disk: null, card: null, sectorHex: '', userText: '', capabilities: 0, opticalEnabled: true, opticalApplied: true, images: [], transfer: null }
  private epoch = 0
  private job = 0
  private upload = 0
  private userTransfer = 0
  private operationClient: MiniConnection | undefined
  private running: Promise<void> | undefined
  private cancelling = false
  constructor(private connection: () => MiniConnection,
    private choose: (type: 'optical' | 'disk' | 'folder') => Promise<string | undefined>,
    private prepare: () => Promise<void>) {}
  private transfer: TransferProgress | undefined
  state(): MiniStorageState { return { ...structuredClone(this.snapshot), transfer: this.transfer?.state() ?? null } }
  cancelLocal(): void { ++this.epoch }
  async stop(): Promise<void> {
    if (this.snapshot.action === '카드 포맷') return // Accepted format continues on the MCU after session closure.
    ++this.epoch; await this.running?.catch(() => {})
  }
  private check(ticket: number): void { if (ticket !== this.epoch) throw new Error('저장소 작업을 취소했습니다.') }
  private client(): MiniConnection {
    const c = this.connection()
    if (this.operationClient && c !== this.operationClient) throw new Error('작업 중 MCU 세션이 바뀌었습니다. 상태를 새로 확인하세요.')
    return c
  }
  private async run(opcode: number, kind: number, payload = new Uint8Array(), ticket = this.epoch, timeout = 3600000): Promise<{ id: number; object: number }> {
    this.check(ticket)
    const reply = await this.client().command(3, opcode, payload)
    if (reply.length !== 4) throw new Error('저장소 job 응답 길이 오류')
    const id = view(reply).getUint32(0, true); this.job = id
    if (!id) throw new Error('잘못된 저장소 job ID')
    const deadline = performance.now() + timeout
    while (performance.now() < deadline) {
      this.check(ticket)
      const result = await this.client().command(0, 7, words(id))
      if (result.length !== 32 || view(result).getUint32(0, true) !== id || result[5] !== kind) throw new Error('저장소 job 불일치')
      const v = view(result), state = result[4]!
      this.snapshot.done = Number(v.getBigUint64(16, true)); this.snapshot.total = Number(v.getBigUint64(24, true))
      if (state !== 0) {
        this.job = 0
        if (state !== 1 || v.getUint16(6, true)) throw new Error(`저장소 작업 실패: 상태 ${state}, 오류 ${v.getUint16(6, true)}, 상세 ${v.getUint32(8, true)}`)
        return { id, object: v.getUint32(12, true) }
      }
      await pause(kind === 11 || kind === 12 ? 5 : 100)
    }
    throw new Error('저장소 작업 완료 대기 시간 초과')
  }
  private async refresh(catalog = false): Promise<void> {
    const c = this.client()
    // NOT_FOUND/BAD_STATE are ordinary empty-slot states; transport and other errors propagate.
    const optional = async (op: number) => c.command(3, op).catch((e: { status?: number }) => { if (e.status === 3 || e.status === 17) return null; throw e })
    const image = await optional(0x305), disk = await optional(0x318), card = await optional(0x308)
    if (image) {
      if (image.length < 48 || image.length !== 48 + view(image).getUint16(14, true)) throw new Error('광학 이미지 정보 길이 오류')
      this.snapshot.image = { id: view(image).getUint32(0, true), size: Number(view(image).getBigUint64(4, true)), mounted: Boolean(image[13]), name: new TextDecoder().decode(image.subarray(48)), crc: view(image).getUint32(16, true) }
      if (!this.snapshot.image.id) this.snapshot.image = null
    } else this.snapshot.image = null
    if (disk) {
      if (disk.length !== 16 || disk[15]) throw new Error('디스크 정보 길이 오류')
      this.snapshot.disk = { id: view(disk).getUint32(0, true), sectors: view(disk).getUint32(4, true), lease: view(disk).getUint32(8, true), format: disk[12]!, state: disk[13]!, writable: Boolean(disk[14]) }
      if (!this.snapshot.disk.lease) this.snapshot.disk = null
    } else this.snapshot.disk = null
    if (card) {
      if (card.length !== 52 || card[1]) throw new Error('카드 정보 길이 오류')
      this.snapshot.card = { present: Boolean(card[0]! & 1), initialized: Boolean(card[0]! & 2), uuid: Buffer.from(card.subarray(8, 24)).toString('hex'), userLength: view(card).getUint16(2, true), revision: view(card).getUint32(4, true) }
    } else this.snapshot.card = null
    const caps = await c.command(3, 0x319).catch((e: { status?: number }) => { if (e.status === 1) return null; throw e })
    if (caps) {
      if (caps.length !== 8 || caps[4]! > 1 || caps[5]! > 1 || caps[6] || caps[7]) throw new Error('저장소 기능 정보 오류')
      this.snapshot.capabilities = view(caps).getUint32(0, true); this.snapshot.opticalEnabled = Boolean(caps[4]); this.snapshot.opticalApplied = Boolean(caps[5])
    }
    if (catalog) await this.listImages()
  }
  private async listImages(): Promise<void> {
    const ticket = this.epoch
    const entries: MiniStorageState['images'] = []
    const image = this.snapshot.image
    if (image) entries.push({ id: image.id, kind: 'optical', name: image.name, size: image.size, format: 0, mounted: image.mounted, path: image.mounted ? 1 : 0, writable: false })
    if (this.snapshot.capabilities & 2) {
      let cursor = 0
      const seen = new Set<number>()
      do {
        this.check(ticket)
        const job = await this.run(0x31b, 15, words(cursor), ticket)
        const reply = await this.client().command(3, 0x31c, words(job.id)), v = view(reply)
        if (reply.length < 8 || reply[4]! > 4 || reply.length !== 8 + reply[4]! * 12 || reply[5] || reply[6] || reply[7]) throw new Error('디스크 목록 응답 오류')
        const next = v.getUint32(0, true)
        if (next && next <= cursor) throw new Error('디스크 목록 커서 오류')
        for (let i = 0; i < reply[4]!; i++) {
          const at = 8 + i * 12, id = v.getUint32(at, true), sectors = v.getUint32(at + 4, true), format = reply[at + 8]!
          if (!id || !sectors || ![1, 2, 3].includes(format) || seen.has(id) || entries.length >= 1024) throw new Error('디스크 목록 범위/중복 오류')
          seen.add(id)
          const d = this.snapshot.disk?.id === id ? this.snapshot.disk : null
          entries.push({ id, kind: 'disk', name: `D${id.toString(16).toUpperCase().padStart(7, '0')}.IMG`, size: sectors * 512, format, mounted: Boolean(d?.lease), path: d?.state === 4 ? 1 : d?.state === 5 ? 2 : 0, writable: d?.writable ?? false })
        }
        cursor = next
      } while (cursor)
    }
    this.snapshot.images = entries
  }
  private async setOptical(enabled: boolean, ticket: number): Promise<void> {
    if (!(this.snapshot.capabilities & 1)) throw new Error('광학 장치 토글을 지원하는 MCU 펌웨어가 필요합니다.')
    if (this.snapshot.opticalEnabled === enabled && this.snapshot.opticalApplied === enabled) return
    if (!enabled && this.snapshot.image?.mounted) await this.run(0x307, 4, words(this.snapshot.image.id), ticket)
    await this.client().command(3, 0x31a, Uint8Array.of(Number(enabled)))
    const until = performance.now() + 15000
    while (performance.now() < until) {
      this.check(ticket); const b = await this.client().command(3, 0x319)
      if (b.length !== 8) throw new Error('광학 장치 상태 응답 오류')
      this.snapshot.opticalEnabled = Boolean(b[4]); this.snapshot.opticalApplied = Boolean(b[5])
      if (this.snapshot.opticalApplied === enabled) return
      await pause(100)
    }
    throw new Error('광학 장치 구성 변경 대기 시간 초과')
  }
  private requireDisk(direct = false, writable = false): NonNullable<MiniStorageState['disk']> {
    const d = this.snapshot.disk
    if (!d?.lease || (direct && d.state !== 3) || (writable && !d.writable)) throw new Error('관리 직접 접근 경로와 필요한 쓰기 권한으로 디스크를 먼저 여세요.')
    return d
  }
  private lba(n: number): number { const d = this.requireDisk(true); if (!number32(n) || n >= d.sectors) throw new Error('LBA 범위 오류'); return n }
  private async writeSector(lease: number, lba: number, data: Uint8Array, ticket: number): Promise<void> {
    if (data.length !== 512) throw new Error('섹터는 정확히 512바이트여야 합니다.')
    const b = new Uint8Array(520); b.set(words(lease, lba)); b.set(data, 8); await this.run(0x315, 12, b, ticket)
  }
  private async optical(filename: string, ticket: number): Promise<void> {
    const f = await fs.open(filename, 'r')
    try {
      const stat = await f.stat(), size = stat.size
      if (!stat.isFile() || size <= 18 * 2048 || size % 2048) throw new Error('2048바이트 단위의 부팅 ISO/UDF 이미지가 필요합니다.')
      await inspectOptical(size, async lba => { const b = Buffer.alloc(2048); if ((await f.read(b, 0, b.length, lba * 2048)).bytesRead !== b.length) throw new Error('광학 이미지가 잘렸습니다.'); return b })
      let checksum = 0
      const block = Buffer.alloc(1024 * 1024)
      this.snapshot.action = '광학 이미지 검사'
      for (let at = 0; at < size; at += block.length) {
        this.check(ticket); const count = Math.min(block.length, size - at)
        if ((await f.read(block, 0, count, at)).bytesRead !== count) throw new Error('이미지가 변경되었습니다.')
        checksum = crc32(block.subarray(0, count), checksum); this.snapshot.done = at + count; this.snapshot.total = size; await pause(0)
      }
      const name = new TextEncoder().encode(path.basename(filename))
      if (!name.length || name.length > 120 || /[\x00-\x1f\x7f-\x9f/\\]/.test(path.basename(filename))) throw new Error('표시 이름은 UTF-8 120바이트 이하여야 합니다.')
      await this.prepare(); this.check(ticket)
      const begin = new Uint8Array(44 + name.length), v = view(begin)
      v.setBigUint64(0, BigInt(size), true); v.setUint32(8, checksum, true); begin[40] = 1; v.setUint16(42, name.length, true); begin.set(name, 44)
      this.upload = (await this.run(0x301, 1, begin, ticket)).object
      if (!this.upload) throw new Error('업로드 ID 누락')
      this.snapshot.action = '광학 이미지 업로드'
      this.snapshot.done = 0; this.snapshot.total = size
      this.transfer = new TransferProgress(size)
      for (let at = 0; at < size;) {
        this.check(ticket)
        const count = Math.min(1008, size - at), data = new Uint8Array(12 + count), header = view(data)
        header.setUint32(0, this.upload, true); header.setBigUint64(4, BigInt(at), true)
        if ((await f.read(data, 12, count, at)).bytesRead !== count) throw new Error('업로드 중 이미지가 잘렸습니다.')
        const reply = await this.client().command(3, 0x302, data)
        at += count
        if (reply.length !== 8 || view(reply).getBigUint64(0, true) !== BigInt(at)) throw new Error('업로드 오프셋 불일치')
        this.snapshot.done = at; this.snapshot.total = size
        this.transfer.advance(at)
      }
      const after = await f.stat()
      if (after.size !== size || after.mtimeMs !== stat.mtimeMs || after.ctimeMs !== stat.ctimeMs) throw new Error('전송 중 원본 이미지가 변경되었습니다.')
      this.snapshot.action = 'SD 기록 검증'
      this.transfer = undefined
      await this.run(0x303, 2, words(this.upload), ticket); this.upload = 0
    } finally { this.transfer = undefined; await f.close() }
  }
  private async createDisk(format: number, ticket: number, filename?: string, folder?: FolderDisk, diskId?: number): Promise<void> {
    const f = filename ? await fs.open(filename, 'r') : undefined
    let lease = 0
    try {
      const stat = await f?.stat(), size = folder ? folder.info.sectors * 512 : stat?.size ?? 0
      if (!size || size % 512 || size / 512 > 0xffffffff || (stat && !stat.isFile())) throw new Error('512바이트 단위의 포맷된 이미지가 필요합니다.')
      const id = diskId ?? randomBytes(4).readUInt32LE() % 0x0fffffff + 1
      if (this.snapshot.image?.mounted) await this.run(0x307, 4, words(this.snapshot.image.id), ticket)
      await this.prepare(); this.check(ticket)
      const begin = new Uint8Array(12); begin.set(words(id, size / 512)); begin[8] = format
      this.snapshot.action = '빈 SD 이미지 생성'
      lease = (await this.run(0x310, 7, begin, ticket)).object
      if (!lease) throw new Error('생성 리스 누락')
      this.snapshot.action = folder ? '폴더 스냅샷 기록' : '디스크 이미지 기록'
      let checksum = 0
      const populated = folder ? folder.populatedSectors : size / 512
      for (let lba = 0; lba < populated; lba += 8) {
        this.check(ticket)
        const count = Math.min(8, populated - lba), b = folder ? await folder.readSectors(lba, count) : new Uint8Array(count * 512)
        if (f && (await f.read(b, 0, b.length, lba * 512)).bytesRead !== b.length) throw new Error('디스크 이미지가 변경되었습니다.')
        checksum = crc32(b, checksum)
        for (let n = 0; n < count; n++) { const sector = b.subarray(n * 512, (n + 1) * 512); if (sector.some(Boolean)) await this.writeSector(lease, lba + n, sector, ticket) }
        this.snapshot.done = (lba + count) * 512; this.snapshot.total = size
        if (!(lba % 1024)) await pause(0)
      }
      // DISK_CREATE zeroes the entire file; preserve its zero tail in the CRC.
      const zero = new Uint8Array(1024 * 1024)
      for (let at = populated * 512; at < size; at += zero.length) { this.check(ticket); checksum = crc32(zero.subarray(0, Math.min(zero.length, size - at)), checksum); await pause(0) }
      if (f && stat) { const after = await f.stat(); if (after.size !== size || after.mtimeMs !== stat.mtimeMs || after.ctimeMs !== stat.ctimeMs) throw new Error('전송 중 이미지가 변경되었습니다.') }
      this.snapshot.action = 'SD 이미지 검증'
      await this.run(0x311, 8, words(lease, checksum), ticket); lease = 0
      this.snapshot.disk = { id, sectors: size / 512, lease: 0, format, state: 0, writable: false }
      // Folder sharing is an immutable read-only snapshot on the target.
      if (folder) { const b = new Uint8Array(8); b.set(words(id)); b[5] = 1; await this.run(0x312, 9, b, ticket) }
    } finally {
      if (lease) { try { await this.run(0x313, 10, words(lease), this.epoch, 30000) } catch { /* Preserve failure; next state query exposes any remaining lease. */ } }
      await f?.close()
    }
  }
  private async abortRemote(): Promise<void> {
    if (this.job) await this.client().command(0, 8, words(this.job))
    else if (this.upload) await this.client().command(0, 9, words(this.upload))
    else if (this.userTransfer) await this.client().command(3, 0x30e, words(this.userTransfer))
  }
  async action(action: MiniStorageAction): Promise<void> {
    if (!action || typeof action.kind !== 'string') throw new Error('저장소 명령 오류')
    if (action.kind === 'cancel') {
      if (!this.running || this.cancelling) return
      if (this.snapshot.action === '카드 포맷') throw new Error('수락된 포맷 작업은 취소할 수 없습니다.')
      this.cancelling = true; ++this.epoch
      try { await this.running.catch(() => {}) }
      finally { this.cancelling = false }
      return
    }
    if (this.running || this.cancelling) throw new Error('다른 저장소 작업이 진행 중입니다.')
    const ticket = ++this.epoch
    this.snapshot.busy = true; this.snapshot.error = ''; this.snapshot.action = action.kind; this.snapshot.done = this.snapshot.total = 0
    this.operationClient = this.connection()
    const work = this.perform(action, ticket)
    this.running = work
    try { await work }
    catch (e) { this.snapshot.error = String(e); try { await this.abortRemote() } catch { /* Link failure expires uncommitted transfers. */ } throw e }
    finally { this.transfer = undefined; this.running = undefined; this.snapshot.busy = false; this.operationClient = undefined }
  }
  private async perform(action: MiniStorageAction, ticket: number): Promise<void> {
    const c = this.client()
    if (action.kind === 'refresh') { await this.refresh(true); return }
    if (action.kind === 'channel-name-set') {
      const name = new TextEncoder().encode(channelName(action.name))
      this.snapshot.action = '채널 이름 저장'
      // Naming depends only on internal card metadata, not optical media or
      // image-catalog support. Do not run an unrelated catalog job afterward.
      const card = await c.command(3, 0x308)
      this.check(ticket)
      if (card.length !== 52 || card[1]) throw new Error('카드 정보 응답 형식 오류')
      if (!(card[0]! & 1)) throw new Error('이름을 저장할 SD 카드가 없습니다.')
      if (!(card[0]! & 2)) throw new Error('SD 내부 식별정보를 준비 중입니다. 잠시 후 다시 시도하세요.')
      if (Buffer.compare(Buffer.from(card.subarray(24, 36)), Buffer.from(card.subarray(36, 48)))) throw new Error('SD 카드에 등록된 MCU UID가 현재 채널과 다릅니다.')
      const payload = new Uint8Array(2 + name.length); view(payload).setUint16(0, name.length, true); payload.set(name, 2)
      try { await this.run(0x31f, 17, payload, ticket, 10000) }
      catch (e) {
        if ((e as { status?: number }).status === 3) throw new Error('채널 이름 저장이 거부됐습니다. 연결된 광학 매체·디스크와 진행 중인 SD 작업을 종료한 뒤 다시 시도하세요.')
        throw e
      }
      return
    }
    if (action.kind === 'optical-upload') { const name = await this.choose('optical'); this.check(ticket); if (name) await this.optical(name, ticket) }
    else if (action.kind === 'disk-upload') { if (![1, 2, 3].includes(action.format)) throw new Error('디스크 포맷 값 오류'); const name = await this.choose('disk'); this.check(ticket); if (name) await this.createDisk(action.format, ticket, name) }
    else if (action.kind === 'folder-share') { const folder = await this.choose('folder'); this.check(ticket); if (folder) await this.createDisk(1, ticket, undefined, await FolderDisk.create(folder)) }
    else {
      await this.refresh(); this.check(ticket)
      if (action.kind === 'optical-set') {
        if (typeof action.enabled !== 'boolean') throw new Error('광학 장치 설정 오류')
        await this.setOptical(action.enabled, ticket)
      } else if (action.kind === 'disk-create' || action.kind === 'disk-format') {
        if (this.snapshot.disk?.lease) throw new Error('연결된 디스크 경로를 안전하게 닫은 뒤 생성/포맷하세요.')
        let sizeMiB: number, id: number | undefined
        if (action.kind === 'disk-format') {
          if (!action.confirmed || !(this.snapshot.capabilities & 4)) throw new Error('이미지 포맷 확인과 지원 펌웨어가 필요합니다.')
          await this.listImages()
          const item = this.snapshot.images.find(i => i.kind === 'disk' && i.id === action.id)
          if (!item) throw new Error('선택한 이미지가 SD 카드에 없습니다.')
          sizeMiB = item.size / 1024 / 1024; id = item.id
        } else sizeMiB = action.sizeMiB
        this.snapshot.action = '파일시스템 이미지 준비'
        const prepared = await makeFormattedImage(action.format, sizeMiB, action.kind === 'disk-format')
        try {
          this.check(ticket)
          if (id !== undefined) await this.run(0x31d, 16, words(id), ticket)
          await this.createDisk(action.format, ticket, prepared.filename, undefined, id)
        } finally { await prepared.close() }
      } else if (action.kind === 'optical-mount' || action.kind === 'optical-unmount') {
        if (!this.snapshot.image?.id) throw new Error('등록된 광학 이미지가 없습니다.')
        if (action.kind === 'optical-mount' && !this.snapshot.opticalApplied) await this.setOptical(true, ticket)
        await this.run(action.kind === 'optical-mount' ? 0x306 : 0x307, action.kind === 'optical-mount' ? 3 : 4, words(this.snapshot.image.id), ticket)
      } else if (action.kind === 'disk-open') {
        if (!number32(action.id) || action.id < 1 || action.id > 0x0fffffff || ![0, 1, 2].includes(action.path) || typeof action.writable !== 'boolean') throw new Error('디스크 ID/경로 오류')
        if (this.snapshot.disk?.lease) throw new Error('기존 경로를 안전하게 분리하고 닫은 뒤 여세요.')
        const b = new Uint8Array(8); b.set(words(action.id)); b[4] = Number(action.writable); b[5] = action.path
        await this.run(0x312, 9, b, ticket)
      } else if (action.kind === 'disk-close' || action.kind === 'disk-sync') {
        const d = this.requireDisk(); await this.run(action.kind === 'disk-close' ? 0x313 : 0x317, action.kind === 'disk-close' ? 10 : 13, words(d.lease), ticket)
      } else if (action.kind === 'disk-read') {
        const lba = this.lba(action.lba), d = this.requireDisk(true), job = await this.run(0x314, 11, words(d.lease, lba), ticket)
        const bytes = new Uint8Array(512)
        for (let part = 0; part < 2; part++) { const b = new Uint8Array(9); b.set(words(d.lease, job.id)); b[8] = part; const data = await c.command(3, 0x316, b); if (data.length !== 256) throw new Error('섹터 응답 길이 오류'); bytes.set(data, part * 256) }
        this.snapshot.sectorHex = Buffer.from(bytes).toString('hex').match(/.{1,32}/g)!.join('\n')
      } else if (action.kind === 'disk-write') {
        if (action.confirmed !== true || typeof action.hex !== 'string' || !/^[0-9a-f\s]+$/i.test(action.hex) || action.hex.replace(/\s/g, '').length !== 1024) throw new Error('확인된 512바이트 HEX 데이터가 필요합니다.')
        const d = this.requireDisk(true, true); await this.writeSector(d.lease, this.lba(action.lba), Buffer.from(action.hex.replace(/\s/g, ''), 'hex'), ticket); await this.run(0x317, 13, words(d.lease), ticket)
      } else if (action.kind === 'card-init') {
        if (this.snapshot.card?.initialized) throw new Error('이미 보드 ID가 설정되어 있습니다.')
        await this.run(0x309, 5, randomBytes(16), ticket)
      } else if (action.kind === 'card-user-read') {
        const card = this.snapshot.card
        if (!card || card.userLength > 4096) throw new Error('유효한 카드 사용자 정보가 없습니다.')
        const data = new Uint8Array(card.userLength)
        for (let at = 0; at < data.length; at += 256) { this.check(ticket); const count = Math.min(256, data.length - at), b = new Uint8Array(8), v = view(b); v.setUint32(0, card.revision, true); v.setUint16(4, at, true); v.setUint16(6, count, true); const part = await c.command(3, 0x30a, b); if (part.length !== count) throw new Error('사용자 정보 길이 오류'); data.set(part, at) }
        this.snapshot.userText = new TextDecoder().decode(data)
      } else if (action.kind === 'card-user-write') {
        if (typeof action.text !== 'string') throw new Error('문자열이 필요합니다.')
        const data = new TextEncoder().encode(action.text)
        if (data.length > 4096) throw new Error('사용자 값은 UTF-8 기준 4096바이트까지 저장합니다.')
        const begin = new Uint8Array(2); view(begin).setUint16(0, data.length, true)
        const reply = await c.command(3, 0x30b, begin)
        if (reply.length !== 4) throw new Error('사용자 값 전송 ID 오류')
        this.userTransfer = view(reply).getUint32(0, true)
        for (let at = 0; at < data.length; at += 1008) { this.check(ticket); const part = data.subarray(at, at + 1008), b = new Uint8Array(6 + part.length); b.set(words(this.userTransfer)); view(b).setUint16(4, at, true); b.set(part, 6); const result = await c.command(3, 0x30c, b); if (result.length !== 2 || view(result).getUint16(0, true) !== at + part.length) throw new Error('사용자 값 오프셋 불일치') }
        await this.run(0x30d, 6, words(this.userTransfer), ticket); this.userTransfer = 0; this.snapshot.userText = action.text
      } else if (action.kind === 'card-format') {
        if (action.confirmed !== true || ![1, 2].includes(action.format)) throw new Error('카드 전체 삭제 확인이 필요합니다.')
        await this.prepare(); this.snapshot.action = '카드 포맷'; await this.run(0x30f, 14, Uint8Array.of(action.format, 0, 0, 0), ticket)
      } else throw new Error('지원하지 않는 저장소 명령입니다.')
    }
    const created = this.snapshot.disk?.lease === 0 ? this.snapshot.disk : null
    await this.refresh(true)
    if (created && !this.snapshot.disk) this.snapshot.disk = created
    this.job = this.upload = this.userTransfer = 0
  }
}

/** Same deliberately limited optical profile as firmware tools/prepare_optical_image.py. */
export async function inspectOptical(size: number, sector: (lba: number) => Promise<Uint8Array>): Promise<void> {
  const blocks = size / 2048
  let iso = false, end = false, vrs = 0, catalog: number | undefined
  for (let lba = 16; lba < Math.min(blocks, 64); lba++) {
    const b = await sector(lba), v = view(b), id = new TextDecoder().decode(b.subarray(1, 6))
    if (id === 'CD001' && b[6] === 1) {
      if (b[0] === 1 && !end) { const n = v.getUint32(80, true); iso = n > 0 && n <= blocks && n === v.getUint32(84, false) && v.getUint16(128, true) === 2048 && v.getUint16(130, false) === 2048 }
      if (b[0] === 255) end = true
      if (b[0] === 0 && new TextDecoder().decode(b.subarray(7, 39)).replace(/[\0 ]+$/, '') === 'EL TORITO SPECIFICATION') {
        const candidate = v.getUint32(71, true); if (catalog !== undefined && candidate !== catalog) throw new Error('충돌하는 부팅 카탈로그'); catalog = candidate
      }
    } else if (b[0] === 0 && b[6] === 1) {
      if (id === 'BEA01') vrs = 1
      else if ((id === 'NSR02' || id === 'NSR03') && vrs === 1) vrs = 2
      else if (id === 'TEA01' && vrs === 2) vrs = 3
    }
  }
  let udf = false
  if (vrs === 3) for (const lba of new Set([256, blocks - 257, blocks - 1])) if (lba >= 256 && lba < blocks) {
    const b = (await sector(lba)).subarray(0, 16), sum = b.reduce((a, n) => a + n, 0) - b[4]!
    if (view(b).getUint16(0, true) === 2 && view(b).getUint32(12, true) === lba && (sum & 255) === b[4]) udf = true
  }
  if (!(iso && end) && !udf) throw new Error('지원하는 ISO 9660/UDF 볼륨 구조가 없습니다.')
  if (catalog === undefined || catalog <= 16 || catalog >= blocks) throw new Error('El Torito 부팅 카탈로그가 없습니다.')
  const b = await sector(catalog), v = view(b)
  let sum = 0, bootable = false
  for (let n = 0; n < 32; n += 2) sum += v.getUint16(n, true)
  if (b[0] !== 1 || b[30] !== 0x55 || b[31] !== 0xaa || (sum & 65535)) throw new Error('부팅 카탈로그 검증 실패')
  const check = (at: number, platform: number) => {
    if (b[at] !== 0x88 || ![0, 0xef].includes(platform) || b[at + 1] !== 0) return
    const lba = v.getUint32(at + 8, true), count = v.getUint16(at + 6, true)
    if (!lba || lba >= blocks || lba * 2048 + count * 512 > size || (!count && platform === 0)) throw new Error('부팅 이미지가 범위를 벗어났습니다.')
    bootable = true
  }
  check(32, b[1]!)
  for (let at = 64; at + 32 <= b.length;) {
    if (b.subarray(at, at + 32).every(n => n === 0)) break
    const kind = b[at], platform = b[at + 1]!, count = v.getUint16(at + 2, true)
    if (![0x90, 0x91].includes(kind!) || !count || at + (count + 1) * 32 > b.length) throw new Error('지원 범위 밖의 부팅 카탈로그입니다.')
    at += 32; const limit = at + count * 32
    for (; at < limit; at += 32) check(at, platform)
    if (kind === 0x91) break
  }
  if (!bootable) throw new Error('BIOS/UEFI no-emulation 부팅 항목이 없습니다.')
}
