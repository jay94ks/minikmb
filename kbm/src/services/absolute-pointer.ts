import type { MiniInput } from '../shared/minikmb'

export interface AbsolutePoint { x: number; y: number }
export interface VideoRect { left: number; top: number; width: number; height: number }
export function absolutePoint(rect: VideoRect, width: number, height: number, clientX: number, clientY: number, dragging = false): AbsolutePoint | undefined {
  if (![rect.left, rect.top, rect.width, rect.height, width, height, clientX, clientY].every(Number.isFinite) ||
    rect.width <= 0 || rect.height <= 0 || width <= 0 || height <= 0) return
  const scale = Math.min(rect.width / width, rect.height / height), w = scale * width, h = scale * height
  const x = (clientX - rect.left - (rect.width - w) / 2) / w, y = (clientY - rect.top - (rect.height - h) / 2) / h
  if (!dragging && (x < 0 || x > 1 || y < 0 || y > 1)) return
  return { x: Math.round(Math.max(0, Math.min(1, x)) * 32767), y: Math.round(Math.max(0, Math.min(1, y)) * 32767) }
}

/** Coalesce movement only between button barriers. Every accepted DOWN/UP keeps its order. */
export class AbsolutePointer {
  private epoch = 0
  private tail: Promise<void> = Promise.resolve()
  private depth = 0
  private pending: { point: AbsolutePoint; epoch: number } | undefined
  constructor(private send: (input: MiniInput) => Promise<void>, private failed: (error: unknown) => void) {}
  cancel(): void { ++this.epoch; this.pending = undefined }
  private enqueue(action: (current: () => boolean) => Promise<void>): void {
    if (this.depth >= 64) { this.cancel(); this.failed(new Error('마우스 입력 대기열이 찼습니다.')); return }
    const epoch = this.epoch; ++this.depth
    const current = () => epoch === this.epoch
    this.tail = this.tail.then(async () => { if (current()) await action(current) })
      .catch(error => { if (current()) { this.cancel(); this.failed(error) } }).finally(() => { --this.depth })
  }
  move(point: AbsolutePoint): void {
    if (this.pending?.epoch === this.epoch) { this.pending.point = point; return }
    const slot = { point, epoch: this.epoch }; this.pending = slot
    this.enqueue(async current => {
      if (this.pending === slot) this.pending = undefined
      if (current()) await this.send({ kind: 'move', ...slot.point })
    })
  }
  button(point: AbsolutePoint | undefined, button: number, down: boolean): void {
    this.pending = undefined
    this.enqueue(async current => {
      if (point) await this.send({ kind: 'move', ...point })
      if (current()) await this.send({ kind: 'button', button, down })
    })
  }
  wheel(vertical: number, horizontal: number): void {
    this.pending = undefined
    this.enqueue(async current => { if (current()) await this.send({ kind: 'wheel', vertical, horizontal }) })
  }
  async idle(): Promise<void> { await this.tail }
}
