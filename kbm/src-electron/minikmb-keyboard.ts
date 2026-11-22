import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import type { MiniInput, MiniKeyboardState } from '../src/shared/minikmb'

export class MiniKeyboard {
  private child: ChildProcessWithoutNullStreams | undefined
  private window = ''
  private uid = ''
  private active = false
  private engaged = false
  private generation = 0
  private request = 0
  private pending = new Map<number, (active: boolean) => void>()
  private ready: Promise<void> | undefined
  private queue: { usage: number; down: boolean; generation: number; uid: string }[] = []
  private pumping = false
  constructor(private helper: string, private allowed: (uid: string) => boolean,
    private send: (uid: string, input: MiniInput, current: () => boolean) => Promise<void>, private release: (uid: string) => Promise<void>,
    private notify: (state: MiniKeyboardState) => void) {}

  private publish(error = ''): MiniKeyboardState { const state = { uid: this.uid, active: this.active, error }; this.notify(state); return state }
  private start(window: string): Promise<void> {
    if (this.child) {
      if (window !== this.window) return Promise.reject(new Error('키보드 캡처 창이 바뀌었습니다.'))
      return this.ready!
    }
    if (!this.helper || !/^[0-9a-f]{1,16}$/i.test(window)) return Promise.reject(new Error('Windows 키보드 캡처 모듈이 없습니다.'))
    this.window = window
    const child = spawn(this.helper, [window, String(process.pid)], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] }); this.child = child
    let bytes = ''
    this.ready = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('키보드 캡처 시작 시간 초과')), 3000)
      child.stdout.on('data', (chunk: Buffer) => {
        if (this.child !== child) return
        bytes += chunk.toString('ascii')
        if (bytes.length > 16384) { void this.fail('키보드 캡처 응답 범위 오류'); return }
        let end: number
        while ((end = bytes.indexOf('\n')) >= 0) {
          const line = bytes.slice(0, end).trim(); bytes = bytes.slice(end + 1)
          if (line === 'READY') { clearTimeout(timer); resolve() }
          else this.receive(line)
        }
      })
      child.once('error', e => { clearTimeout(timer); reject(e); if (this.child === child) void this.fail('키보드 캡처 모듈 실행 실패') })
      child.once('close', () => { clearTimeout(timer); reject(new Error('키보드 캡처 모듈 종료')); if (this.child === child) { this.child = undefined; void this.fail('키보드 캡처가 종료되었습니다.') } })
      child.stdin.on('error', () => { if (this.child === child) void this.fail('키보드 캡처 연결 종료') })
      child.stderr.resume() // Never log key data or helper output.
    })
    return this.ready
  }

  private receive(line: string): void {
    const status = /^S (\d+) ([01])$/.exec(line)
    if (status) {
      const generation = Number(status[1]), resolve = this.pending.get(generation), active = status[2] === '1'
      if (resolve && generation === this.generation) this.active = active
      resolve?.(active); return
    }
    const reset = /^R (\d+) 0$/.exec(line)
    if (reset) { if (Number(reset[1]) === this.generation && this.active) void this.deactivate(); return }
    const key = /^K (\d+) (\d+) ([01])$/.exec(line)
    if (!key) { void this.fail('키보드 캡처 응답 형식 오류'); return }
    const generation = Number(key[1]), usage = Number(key[2])
    if (!this.active || generation !== this.generation) return
    if (!this.allowed(this.uid)) { void this.deactivate(); return }
    if (!((usage >= 4 && usage <= 0xa4) || (usage >= 0xe0 && usage <= 0xe7)) || this.queue.length >= 64) { void this.fail('키보드 입력 큐 오류'); return }
    this.queue.push({ usage, down: key[3] === '1', generation, uid: this.uid }); void this.pump()
  }
  private async pump(): Promise<void> {
    if (this.pumping) return
    this.pumping = true
    try {
      while (this.queue.length) {
        const key = this.queue.shift()!
        const current = () => this.active && key.generation === this.generation && this.allowed(key.uid)
        if (current()) await this.send(key.uid, { kind: 'key', usage: key.usage, down: key.down }, current)
      }
    } catch { await this.fail('타깃 키 입력 전송 실패 · 입력을 해제했습니다.') }
    finally { this.pumping = false }
  }
  async set(uid: string, enabled: boolean, window: string): Promise<MiniKeyboardState> {
    const ticket = ++this.request
    await this.deactivate(false)
    if (!enabled || ticket !== this.request) return this.publish()
    if (!this.allowed(uid)) throw new Error('현재 제어 중인 타깃이 아닙니다.')
    try {
      await this.start(window)
      if (ticket !== this.request || !this.allowed(uid)) return this.publish()
      this.uid = uid
      this.engaged = true
      const generation = ++this.generation
      const active = await new Promise<boolean>((resolve, reject) => {
        const timer = setTimeout(() => { this.pending.delete(generation); reject(new Error('키보드 캡처 설정 시간 초과')) }, 2000)
        this.pending.set(generation, value => { this.pending.delete(generation); clearTimeout(timer); resolve(value) })
        this.child!.stdin.write(`E ${generation}\n`)
      })
      if (ticket !== this.request) return this.publish()
      if (!active) throw new Error('영상 창에 포커스를 둔 뒤 다시 시도하세요.')
      this.active = true; return this.publish()
    } catch (e) { if (ticket === this.request) await this.fail(e instanceof Error ? e.message : String(e)); throw e }
  }
  async deactivate(cancelRequest = true): Promise<MiniKeyboardState> {
    if (cancelRequest) ++this.request
    const uid = this.uid
    const release = this.engaged; this.engaged = false
    this.active = false; ++this.generation; this.queue.length = 0
    for (const resolve of this.pending.values()) resolve(false)
    this.pending.clear()
    if (this.child?.stdin.writable) this.child.stdin.write(`D ${this.generation}\n`)
    this.publish()
    if (uid && release) await this.release(uid).catch(() => {})
    return { uid, active: false, error: '' }
  }
  private async fail(message: string): Promise<void> {
    await this.deactivate(); this.publish(message)
    const child = this.child; this.child = undefined; this.ready = undefined
    child?.stdin.end(); child?.kill()
  }
  check(): void { if (this.active && !this.allowed(this.uid)) void this.deactivate() }
  async stop(): Promise<void> {
    await this.deactivate()
    const child = this.child; this.child = undefined; this.ready = undefined; this.window = ''
    if (child) {
      await new Promise<void>(resolve => {
        const timer = setTimeout(() => { child.kill(); resolve() }, 1000)
        child.once('close', () => { clearTimeout(timer); resolve() }); child.stdin.end()
      })
    }
  }
}
