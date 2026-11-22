interface SerialPortInfo {
  usbVendorId?: number
  usbProductId?: number
}

interface SerialOptions {
  baudRate: number
  dataBits?: 7 | 8
  stopBits?: 1 | 2
  parity?: 'none' | 'even' | 'odd'
  bufferSize?: number
  flowControl?: 'none' | 'hardware'
}

interface SerialPort extends EventTarget {
  readonly readable: ReadableStream<Uint8Array> | null
  readonly writable: WritableStream<Uint8Array> | null
  getInfo(): SerialPortInfo
  open(options: SerialOptions): Promise<void>
  close(): Promise<void>
}

interface SerialPortRequestOptions {
  filters?: Array<{
    usbVendorId?: number
    usbProductId?: number
  }>
}

interface SerialConnectionEvent extends Event {
  readonly target: SerialPort
}

interface Serial extends EventTarget {
  getPorts(): Promise<SerialPort[]>
  requestPort(options?: SerialPortRequestOptions): Promise<SerialPort>
  addEventListener(
    type: 'connect' | 'disconnect',
    listener: (event: SerialConnectionEvent) => void
  ): void
  removeEventListener(
    type: 'connect' | 'disconnect',
    listener: (event: SerialConnectionEvent) => void
  ): void
}

interface Navigator {
  readonly serial: Serial
}

