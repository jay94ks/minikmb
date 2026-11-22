import { randomBytes } from 'node:crypto'
import { MiniDecoder, miniEncode, type MiniFrame } from '../src/services/minikmb-protocol'
export interface MiniWire { write(data: Uint8Array): Promise<void>; close(): void }
interface Pending { opcode: number; sequence: number; session: bigint; resolve(frame: MiniFrame): void; reject(error: Error): void; timer: ReturnType<typeof setTimeout> }
export class MiniConnection {
  private decoder = new MiniDecoder()
  private session = 0n
  private sequence = [1, 1, 1, 1, 1, 1]
  private tails = Array.from({ length: 6 }, () => Promise.resolve())
  private depths = [0, 0, 0, 0, 0, 0]
  private pending = new Map<number, Pending>()
  private done = new Map<number, number>()
  private waiters = new Map<number, { resolve(): void; reject(e: Error): void; timer: ReturnType<typeof setTimeout> }>()
  private closed = false
  private listeners = new Set<(frame: MiniFrame) => void>()
  features = 0
  constructor(public uid: string, private wire: MiniWire, private readonly failed: (error: string) => void) {}
  feed(bytes: Uint8Array): void {
    for (const frame of this.decoder.feed(bytes)) {
      if (frame.session === this.session && frame.kind === 3 && frame.channel === 1 && frame.opcode === 0x180 && frame.payload.length === 8) {
        const v = new DataView(frame.payload.buffer), ticket = v.getUint32(0, true), status = v.getUint16(4, true)
        const waiter = this.waiters.get(ticket)
        if (waiter) { clearTimeout(waiter.timer); this.waiters.delete(ticket); status ? waiter.reject(new Error(`HID 완료 오류 ${status}`)) : waiter.resolve() }
        else { if (this.done.size >= 64) this.done.delete(this.done.keys().next().value!); this.done.set(ticket, status) }
      }
      const p = this.pending.get(frame.channel)
      if (frame.kind === 2 && p && p.opcode === frame.opcode && p.sequence === frame.sequence &&
          ((frame.channel === 0 && frame.opcode === 1) || frame.session === p.session)) {
        clearTimeout(p.timer); this.pending.delete(frame.channel); p.resolve(frame)
      }
      if (frame.session === this.session && (frame.kind === 3 || frame.kind === 4)) {
        try { for (const listener of this.listeners) listener(frame) }
        catch { this.close('관리 STREAM/EVENT 처리 오류'); return }
      }
    }
  }
  subscribe(listener: (frame: MiniFrame) => void): () => void { this.listeners.add(listener); return () => this.listeners.delete(listener) }
  private exchange(channel: number, opcode: number, payload: Uint8Array, session: bigint, sequence: number): Promise<MiniFrame> {
    if (this.closed) return Promise.reject(new Error('MCU 연결이 끊어졌습니다.'))
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => this.close('관리 응답 시간 초과 · 입력을 자동 재시도하지 않습니다.'), 1000)
      this.pending.set(channel, { opcode, sequence, session, resolve, reject, timer })
      void this.wire.write(miniEncode({ kind: 1, channel, opcode, payload, session, sequence })).catch(() => this.close('관리 USB 쓰기 실패'))
    })
  }
  async hello(): Promise<void> {
    const nonce = randomBytes(8), payload = new Uint8Array(12)
    payload.set([1, 1], 0); payload.set(nonce, 4)
    const frame = await this.exchange(0, 1, payload, 0n, 1)
    const result = this.result(frame)
    const foundUid = Buffer.from(result.slice(8, 20)).toString('hex').toUpperCase()
    if (!frame.session || result.length !== 40 || Buffer.from(result.slice(0, 8)).compare(nonce) ||
        (this.uid && foundUid !== this.uid) || !/^[0-9A-F]{24}$/.test(foundUid)) throw new Error('MCU HELLO UID/nonce 불일치')
    // Windows serialport may expose the composite interface's location instance,
    // rather than iSerialNumber. Only allowlisted management ports reach here.
    this.uid = foundUid
    this.session = frame.session; this.features = new DataView(result.buffer, result.byteOffset).getUint32(28, true)
    const info = await this.command(0, 3)
    if (Buffer.compare(Buffer.from(info), Buffer.from(result))) throw new Error('MCU INFO 불일치')
  }
  private result(frame: MiniFrame): Uint8Array {
    if (frame.payload.length < 8) throw new Error('잘못된 관리 응답')
    const v = new DataView(frame.payload.buffer, frame.payload.byteOffset)
    if (v.getUint16(2, true)) throw new Error('잘못된 관리 응답 예약 필드')
    const status = v.getUint16(0, true)
    if (status) throw Object.assign(new Error(`MCU 응답 오류 ${status}`), { status })
    return frame.payload.slice(8)
  }
  command(channel: number, opcode: number, payload: Uint8Array = new Uint8Array(), current = () => true): Promise<Uint8Array> {
    if (!Number.isInteger(channel) || channel < 0 || channel > 5 || this.depths[channel]! >= 32) return Promise.reject(new Error('관리 요청 대기열이 찼습니다.'))
    this.depths[channel]!++
    const action = this.tails[channel]!.then(async () => {
      if (!current()) throw new Error('채널 전환으로 이전 입력 취소')
      const sequence = this.sequence[channel]!
      if (!this.session || sequence >= 0xfffffffe) throw new Error('MCU 세션을 다시 연결하세요.')
      const frame = await this.exchange(channel, opcode, payload, this.session, sequence)
      if (frame.payload.length >= 2) {
        const status = new DataView(frame.payload.buffer).getUint16(0, true)
        if (![5, 6, 7, 8].includes(status)) this.sequence[channel] = sequence + 1
        else this.close(`관리 세션/요청 순서 불일치 (상태 ${status}, 채널 ${channel}, 명령 0x${opcode.toString(16)}, 번호 ${sequence})`)
      }
      return this.result(frame)
    })
    this.tails[channel] = action.then(() => {}, () => {}).finally(() => { this.depths[channel]!-- })
    return action
  }
  async hid(opcode: number, payload: Uint8Array, current: () => boolean): Promise<void> {
    if (!(this.features & 1)) throw new Error('이 MCU 펌웨어는 HID가 비활성 상태입니다.')
    let result: Uint8Array
    for (let attempt = 0;; attempt++) {
      try { result = await this.command(1, opcode, payload, current); break }
      catch (e) {
        if (opcode !== 0x107 || attempt >= 4 || (e as { status?: number }).status !== 4) throw e
        await new Promise(resolve => setTimeout(resolve, 20)) // Only idempotent RELEASE_ALL is retried.
      }
    }
    if (result.length !== 4) throw new Error('잘못된 HID ticket')
    const ticket = new DataView(result.buffer).getUint32(0, true)
    if (this.done.has(ticket)) {
      const status = this.done.get(ticket); this.done.delete(ticket)
      if (status) throw new Error(`HID 완료 오류 ${status}`)
      return
    }
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => { this.waiters.delete(ticket); reject(new Error('HID 완료 미확인')) }, 1000)
      this.waiters.set(ticket, { resolve, reject, timer })
    })
  }
  close(error = 'MCU 연결 종료'): void {
    if (this.closed) return
    this.closed = true; this.session = 0n
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(new Error(error)) }
    for (const p of this.waiters.values()) { clearTimeout(p.timer); p.reject(new Error(error)) }
    this.pending.clear(); this.waiters.clear(); this.done.clear(); this.wire.close(); this.failed(error)
    this.listeners.clear()
  }
}
