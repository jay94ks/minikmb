/** One owner, including an in-flight getUserMedia request, across rapid switches. */
export class SingleCapture<T> {
  private epoch = 0
  private tail: Promise<void> = Promise.resolve()
  private current: T | undefined
  constructor(private readonly open: (id: string) => Promise<T>, private readonly stop: (value: T) => void) {}
  select(id?: string): Promise<T | undefined> {
    const epoch = ++this.epoch
    const result = this.tail.then(async () => {
      if (this.current !== undefined) { this.stop(this.current); this.current = undefined }
      if (epoch !== this.epoch || !id) return undefined
      const value = await this.open(id)
      if (epoch !== this.epoch) { this.stop(value); return undefined }
      this.current = value; return value
    })
    this.tail = result.then(() => {}, () => {})
    return result
  }
}
