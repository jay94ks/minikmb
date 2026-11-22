import type { MiniTransferProgress } from '../src/shared/mini-tools'

// Payload throughput, measured only after a valid device offset ACK.
export class TransferProgress {
  private started: number
  private updated: number
  private done = 0
  private samples: { time: number; done: number }[]
  constructor(private total: number, private clock = () => performance.now()) {
    this.started = this.updated = clock(); this.samples = [{ time: this.started, done: 0 }]
  }
  advance(done: number): void {
    if (!Number.isSafeInteger(done) || done < this.done || done > this.total) throw new Error('전송 진행량 범위 오류')
    const now = this.clock()
    if (done > this.done) this.updated = now
    this.done = done
    if (now - this.samples.at(-1)!.time >= 250) this.samples.push({ time: now, done })
    this.trim(now)
  }
  private trim(now: number): void {
    while (this.samples.length > 1 && this.samples[1]!.time <= now - 5000) this.samples.shift()
    while (this.samples.length > 24) this.samples.shift()
  }
  state(): MiniTransferProgress {
    const now = this.clock(); this.trim(now)
    const elapsedSeconds = Math.max(0, now - this.started) / 1000, base = this.samples[0]!
    if (elapsedSeconds < 1 && this.done < this.total) return { bytesPerSecond: null, remainingSeconds: null, elapsedSeconds }
    const seconds = (now - Math.max(base.time, now - 5000)) / 1000
    const rate = now - this.updated >= 5000 || seconds <= 0 ? 0 : Math.max(0, (this.done - base.done) / seconds)
    return { bytesPerSecond: rate, remainingSeconds: this.done >= this.total ? 0 : rate > 0 ? (this.total - this.done) / rate : null, elapsedSeconds }
  }
}
