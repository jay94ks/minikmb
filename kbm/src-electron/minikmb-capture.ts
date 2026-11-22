import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import type { MiniVideoInfo, MiniVideoFrame, MiniVideoMode } from '../src/shared/mini-capture'

interface Packet { width: number; height: number; frameRate: number; jpeg: Uint8Array }
export class MiniFrameParser {
  private pending = Buffer.alloc(0)
  feed(chunk: Uint8Array): Packet[] {
    if (this.pending.length + chunk.length > 8 * 1024 * 1024 + 65536) throw new Error('영상 프레임 버퍼 한도 초과')
    this.pending = Buffer.concat([this.pending, chunk]); const out: Packet[] = []
    while (this.pending.length >= 32) {
      const b = this.pending, length = b.readUInt32LE(4), width = b.readUInt32LE(8), height = b.readUInt32LE(12)
      const n = b.readUInt32LE(16), d = b.readUInt32LE(20)
      if (b.readUInt32LE(0) !== 0x4d4a5047 || length < 4 || length > 8 * 1024 * 1024 || width < 2 || width > 8192 || height < 2 || height > 8192 || !d || n / d > 240) throw new Error('영상 프레임 헤더 오류')
      if (b.length < 32 + length) break
      const jpeg = Uint8Array.from(b.subarray(32, 32 + length))
      if (jpeg[0] !== 255 || jpeg[1] !== 216) throw new Error('MJPEG 프레임 오류')
      out.push({ width, height, frameRate: n / d, jpeg }); this.pending = b.subarray(32 + length)
    }
    return out
  }
}
interface CaptureSession { child: ChildProcessWithoutNullStreams; info: MiniVideoInfo; closed: Promise<void>; root: string }
export class MiniCapture {
  private epoch = 0
  private token = 0
  private sequence = 0
  private inflight: { token: number; sequence: number } | undefined
  private latest: MiniVideoFrame | undefined
  private queue: Promise<void> = Promise.resolve()
  private running: CaptureSession | undefined
  constructor(private helper: string, private send: (frame: MiniVideoFrame) => void, private ended: (uid: string, message: string) => void) {}
  private emit(frame: MiniVideoFrame): void {
    if (this.inflight) {
      if (this.latest && this.running) this.running.info.dropped++
      frame.dropped = this.running?.info.dropped ?? frame.dropped; this.latest = frame; return
    }
    this.inflight = { token: frame.token, sequence: frame.sequence }; this.send(frame)
  }
  ack(token: unknown, sequence: unknown): void {
    if (token !== this.inflight?.token || sequence !== this.inflight?.sequence) return
    this.inflight = undefined
    const next = this.latest; this.latest = undefined
    if (next && next.token === this.running?.info.token) this.emit(next)
  }
  private async closeSession(): Promise<void> {
    const session = this.running; this.running = undefined; this.inflight = undefined; this.latest = undefined
    if (!session) return
    session.child.stdin.end('q')
    const timer = setTimeout(() => session.child.kill(), 2500)
    try { await session.closed } finally { clearTimeout(timer) }
    await new Promise(resolve => setTimeout(resolve, 150))
  }
  private async attempt(uid: string, root: string, mode: number[], epoch: number, requestId: number): Promise<CaptureSession> {
    const token = ++this.token
    const child = spawn(this.helper, [root, ...mode.map(String)], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] })
    let exit = () => {}; const closed = new Promise<void>(resolve => { exit = resolve })
    const session: CaptureSession = { child, info: { uid, token, requestId, width: 0, height: 0, frameRate: 0, frames: 0, dropped: 0 }, closed, root }
    this.running = session
    let errorText = '', ready = false
    const parser = new MiniFrameParser()
    child.stderr.on('data', (data: Buffer) => { errorText = (errorText + data.toString('utf8')).slice(-4096) })
    child.stdin.on('error', () => {})
    return new Promise<CaptureSession>((resolve, reject) => {
      const timer = setTimeout(() => { reject(new Error('5초 안에 영상 첫 프레임을 받지 못했습니다.')); child.stdin.end('q') }, 6500)
      child.on('error', e => { clearTimeout(timer); exit(); reject(e) })
      child.on('close', code => {
        clearTimeout(timer); exit()
        if (!ready) reject(new Error(`영상 열기 실패 (${code}): ${errorText.trim()}`))
        else if (this.running === session && this.epoch === epoch) this.ended(uid, '영상 스트림이 종료됐습니다.')
      })
      child.stdout.on('data', (data: Buffer) => {
        try {
          for (const packet of parser.feed(data)) {
            if (epoch !== this.epoch || this.running !== session) continue
            Object.assign(session.info, { width: packet.width, height: packet.height, frameRate: packet.frameRate })
            session.info.frames++
            if (!ready) { ready = true; clearTimeout(timer); resolve(session) }
            this.emit({ ...session.info, sequence: ++this.sequence, jpeg: packet.jpeg })
          }
        } catch (e) { clearTimeout(timer); reject(e); child.kill(); if (ready) this.ended(uid, String(e)) }
      })
    })
  }
  start(uid: string, root: string, mode: MiniVideoMode, requestId: number): Promise<MiniVideoInfo> {
    if (!Number.isSafeInteger(requestId) || requestId < 1 || requestId > 0xffffffff) return Promise.reject(new Error('영상 선택 번호 오류'))
    if (typeof mode === 'string' ? !['auto', '1080', '720', '480'].includes(mode) : !mode ||
      !Number.isInteger(mode.width) || !Number.isInteger(mode.height) || mode.width < 2 || mode.width > 8192 || mode.height < 2 || mode.height > 8192 ||
      !Number.isFinite(mode.frameRate) || mode.frameRate < 1 || mode.frameRate > 240) return Promise.reject(new Error('영상 모드 오류'))
    const epoch = ++this.epoch
    const result = this.queue.then(async () => {
      const previous = this.running?.info.width && this.running.root === root && this.running.info.uid === uid ? { ...this.running.info } : undefined
      await this.closeSession()
      if (epoch !== this.epoch) throw new Error('이전 영상 선택 취소')
      const sizes: Record<string, number[]> = { '1080': [1920, 1080], '720': [1280, 720], '480': [640, 480] }
      const candidates = mode === 'auto' ? [[1920, 1080], [1280, 720], [640, 480]].flatMap(size => [30, 24, 0].map(fps => [...size, fps])).concat([[0, 0, 0]]) : typeof mode === 'object' ? [[mode.width, mode.height, mode.frameRate]] : [[...sizes[mode]!, 30]]
      let failure: unknown
      for (const candidate of candidates) {
        if (epoch !== this.epoch) break
        try { const session = await this.attempt(uid, root, candidate, epoch, requestId); return { ...session.info } }
        catch (e) { failure = e; await this.closeSession() }
      }
      if (previous && mode !== 'auto' && epoch === this.epoch) {
        try { await this.attempt(uid, root, [previous.width, previous.height, previous.frameRate], epoch, requestId) }
        catch { await this.closeSession() }
      }
      throw failure ?? new Error('영상 선택 취소')
    })
    this.queue = result.then(() => {}, () => {})
    return result
  }
  async stop(): Promise<void> {
    ++this.epoch
    const stop = this.queue.then(() => this.closeSession())
    // Interrupt an opening process immediately; queued cleanup waits for its close.
    this.running?.child.stdin.end('q')
    this.queue = stop.then(() => {}, () => {}); await stop
  }
}
