// No physical USB/UART/HID traffic. Real lwIP peers live entirely in memory.
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const assert = require('node:assert/strict')
const net = require('node:net')
const dgram = require('node:dgram')
const { createRequire } = require('node:module')
const { pathToFileURL } = require('node:url')
const root = path.resolve(__dirname, '..')
const ts = require(path.join(root, 'kbm/node_modules/typescript'))
const modules = new Map()
let tcpip
function load(relative) {
  const filename = path.resolve(root, relative)
  if (modules.has(filename)) return modules.get(filename)
  const exports = {}; modules.set(filename, exports)
  const localRequire = createRequire(filename)
  const requireModule = name => name === 'tcpip' ? tcpip : name.startsWith('.') ?
    load(path.relative(root, path.resolve(path.dirname(filename), name + '.ts'))) : localRequire(name)
  const js = ts.transpileModule(fs.readFileSync(filename, 'utf8'), { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true
  } }).outputText
  vm.runInThisContext(`(function(exports,require){${js}\n})`, { filename })(exports, requireModule)
  return exports
}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
async function timeout(promise, ms = 10000) {
  let timer
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('test timeout')), ms) })]) }
  finally { clearTimeout(timer) }
}
function testEthernet() {
  const { fragmentFrame, EthernetAssembler, crc32 } = load('kbm/src/services/ethernet.ts')
  assert.equal(crc32(new TextEncoder().encode('123456789')), 0xcbf43926)
  for (const n of [14, 64, 65, 1514]) {
    const frame = Uint8Array.from({ length: n }, (_, i) => i * 47 & 255)
    const chunks = fragmentFrame(frame, 65535), a = new EthernetAssembler()
    let result
    for (const chunk of chunks) { assert(chunk.length <= 74); result = a.accept(chunk) }
    assert.deepEqual(result, frame)
    const corrupt = chunks.map(c => c.slice()); corrupt[corrupt.length - 1][10] ^= 1
    const b = new EthernetAssembler()
    for (const chunk of corrupt) assert.equal(b.accept(chunk), undefined)
    if (chunks.length > 1) {
      const c = new EthernetAssembler()
      assert.equal(c.accept(chunks[1]), undefined)
    }
  }
  assert.throws(() => fragmentFrame(new Uint8Array(1515), 0))
  console.log('PASS Ethernet fragmentation, CRC, malformed/out-of-order/maximum frames')
}
function testDhcp() {
  const { DhcpServer, internetChecksum } = load('kbm/src-electron/dhcp.ts')
  const { defaultNetworkConfig, validateNetworkConfig, validateForwardRule } = load('kbm/src/shared/network.ts')
  const config = { ...defaultNetworkConfig }
  assert.throws(() => validateNetworkConfig({ ...config, poolStart: 1 }))
  assert.throws(() => validateForwardRule({ port: 0, protocol: 'tcp', targetIp: '8.8.8.8', listenPort: 22, targetPort: 22 }, config))
  function request(kind, options = [], mac = 1) {
    const frame = new Uint8Array(42 + 300), view = new DataView(frame.buffer)
    frame.fill(255, 0, 6); frame.set([2, 0, 0, 0, 0, mac], 6)
    view.setUint16(12, 0x800); frame[14] = 0x45; view.setUint16(16, frame.length - 14)
    frame[22] = 64; frame[23] = 17; frame.fill(255, 30, 34)
    view.setUint16(24, internetChecksum(frame.subarray(14, 34)))
    view.setUint16(34, 68); view.setUint16(36, 67); view.setUint16(38, 308)
    frame.set([1, 1, 6, 0, 0x12, 0x34, 0x56, 0x78], 42)
    frame.set([2, 0, 0, 0, 0, mac], 70)
    frame.set([99, 130, 83, 99, 53, 1, kind, ...options, 255], 278)
    return frame
  }
  const dhcp = new DhcpServer(config, Uint8Array.of(2, 0, 0, 0, 0, 1), 100)
  const offer = dhcp.handle(request(1), 1000)
  assert(offer); assert.deepEqual([...offer.subarray(58, 62)], [192, 168, 127, 100])
  assert.equal(offer[284], 2); assert.equal(internetChecksum(offer.subarray(14, 34)), 0)
  const ack = dhcp.handle(request(3, [50, 4, 192, 168, 127, 100, 54, 4, 192, 168, 127, 1]), 1500)
  assert(ack); assert.equal(ack[284], 5)
  const codes = []
  for (let at = 282; ack[at] !== 255;) { codes.push(ack[at]); at += 2 + ack[at + 1] }
  assert(codes.includes(3) && !codes.includes(6) && !codes.includes(121))
  let router
  for (let at = 282; ack[at] !== 255;) {
    if (ack[at] === 3) router = [...ack.subarray(at + 2, at + 2 + ack[at + 1])]
    at += 2 + ack[at + 1]
  }
  assert.deepEqual(router, [192, 168, 127, 1])
  const sharedDhcp = new DhcpServer(config, Uint8Array.of(2, 0, 0, 0, 0, 1), 100, config.serverIp)
  const sharedOffer = sharedDhcp.handle(request(1), 1000); let sharedDns
  for (let at = 282; sharedOffer[at] !== 255;) { if (sharedOffer[at] === 6) sharedDns = [...sharedOffer.subarray(at + 2, at + 6)]; at += 2 + sharedOffer[at + 1] }
  assert.deepEqual(sharedDns, [192, 168, 127, 1], 'DNS is advertised only in explicit sharing mode')
  assert.equal(dhcp.handle(request(3, [54, 4, 192, 168, 127, 99]), 2000), undefined)
  const nak = dhcp.handle(request(3, [50, 4, 192, 168, 127, 150]), 2000)
  assert.equal(nak[284], 6)
  const bad = request(1); bad[24] ^= 1
  assert.equal(dhcp.handle(bad), undefined)
  const malformed = request(1); malformed[285] = 70; malformed[286] = 255
  assert.equal(dhcp.handle(malformed), undefined)
  console.log('PASS DHCP discover/offer/request/ack/nak, bounded leases, gateway .1, no DNS, validation')
}
async function testMaster() {
  const { encodePacket, ProgressiveDecoder } = load('kbm/src/services/protocol.ts')
  const { MasterSerialClient } = load('kbm/src/services/master-serial.ts')
  const writes = [], decoder = new ProgressiveDecoder()
  let deliver
  global.window = { kbmNative: { master: {
    connect: async () => true, onData: listener => { deliver = listener },
    onConnection: () => {}, onError: () => {}, write: async bytes => {
      const packets = decoder.feed(bytes).filter(e => e.type === 'packet').map(e => e.packet)
      for (const packet of packets) {
        writes.push(packet)
        if (packet.op === 0x71) deliver(encodePacket({ op: 0xf1, payload: Uint8Array.from({ length: 21 }, (_, i) => i === 1 ? 3 : i === 2 ? 3 : 0) }))
        if (packet.op === 0x77) deliver(encodePacket({ op: 0xf7, payload: Uint8Array.of(0, packet.payload[0] ?? 0, packet.payload[1] ?? 1, 0, 0) }))
      }
    }
  } } }
  const client = new MasterSerialClient()
  assert(await client.initialize())
  function ack(packet) {
    const p = packet.payload
    deliver(encodePacket({ op: 0xf3, payload: Uint8Array.of(p[0], p[1], p[2], 0, p[3] | 128, 0, 0) }))
  }
  const a = client.route(0, { op: 0x40, payload: Uint8Array.of(1) })
  const first = writes.at(-1)
  const bulk = client.route(0, { op: 0x40, payload: Uint8Array.of(2) })
  const hid = client.keyDown(0, 4)
  const other = client.keyDown(1, 5)
  assert.equal(writes.at(-1).payload[0], 1)
  ack(writes.at(-1)); await other
  let serialReceived = false
  client.onSerialData((port, bytes) => { assert.equal(port, 1); assert.equal(bytes[0], 55); serialReceived = true })
  deliver(encodePacket({ op: 0xf4, payload: Uint8Array.of(1, 1, 0, 55) }))
  assert(serialReceived)
  ack(first); await a
  assert.equal(writes.at(-1).payload[3], 0x10, 'HID must bypass queued Ethernet fragments')
  ack(writes.at(-1)); await hid
  assert.equal(writes.at(-1).payload[3], 0x40)
  // A stale ACK from the previous request cannot complete the active fragment.
  ack(first); assert(client.active[0])
  ack(writes.at(-1)); await bulk
  const native = await client.setNativeMode(true, false)
  assert(native.enabled && !native.dhcp)
  assert.deepEqual([...writes.at(-1).payload], [1, 0])
  await assert.rejects(client.networkEnable(0, true), /네이티브/)
  await client.setNativeMode(false, true)
  const reboot = client.route(1, { op: 0x60, payload: Uint8Array.of(82, 80, 50, 48) })
  assert.equal(writes.at(-1).payload[3], 0x60); ack(writes.at(-1)); await reboot
  client.handleDisconnected()
  console.log('PASS dual UART concurrency, HID priority, fast ACK, stale ACK, async serial interleave')
}
async function testNativeEthernet() {
  const { EthernetTransport }=load('kbm/src-electron/ethernet-transport.ts')
  const { RoutedTransport }=load('kbm/src-electron/routed-transport.ts')
  const { ProgressiveDecoder }=load('kbm/src/services/protocol.ts')
  const { fragmentFrame,EthernetAssembler }=load('kbm/src/services/ethernet.ts')
  const decoder=new ProgressiveDecoder(),packets=[],received=[],frames=[new EthernetAssembler(),new EthernetAssembler()]
  const transport=new RoutedTransport(async bytes=>{
    for(const item of decoder.feed(bytes))if(item.type==='packet') {
      const p=item.packet;packets.push(p)
      const frame=frames[p.payload[0]].accept(p.payload.subarray(4))
      if(frame)received.push({port:p.payload[0],frame})
      queueMicrotask(()=>transport.accept({op:0xf3,payload:Uint8Array.of(p.payload[0],p.payload[1],p.payload[2],0,0xc0,0,0)}))
    }
  })
  const incoming=[],native=new EthernetTransport(transport,(port,frame)=>incoming.push({port,frame}))
  const data=Uint8Array.from({length:1514},(_,i)=>[2,3,27,255,0][i%5])
  await Promise.all([0,1].map(port=>native.send(port,data)))
  assert.equal(received.length,2);for(const item of received)assert.deepEqual(item.frame,data)
  assert.equal(packets.length,48);assert(packets.every(p=>p.payload[3]===0x40&&p.payload.length<=78))
  for(const chunk of fragmentFrame(data,3))native.accept({op:0xf6,payload:Uint8Array.of(1,...chunk)})
  assert.equal(incoming.length,1);assert.deepEqual(incoming[0],{port:1,frame:data})
  const corrupt=fragmentFrame(data,4);corrupt.at(-1)[10]^=1
  for(const chunk of corrupt)native.accept({op:0xf6,payload:Uint8Array.of(0,...chunk)})
  assert.equal(incoming.length,1,'CRC corrupt frames never reach native network stack')
  const partial=fragmentFrame(data,5);native.accept({op:0xf6,payload:Uint8Array.of(0,...partial[0])});native.cancel(0)
  for(const chunk of partial.slice(1))native.accept({op:0xf6,payload:Uint8Array.of(0,...chunk)})
  assert.equal(incoming.length,1,'Canceled partial frame cannot complete')
  console.log('PASS native Ethernet transport: both UARTs,64-byte priority boundaries, actual ACKs, CRC reassembly and cancellation, zero renderer IPC')
}
async function testShutdownOrdering() {
  const {NetworkService}=load('kbm/src-electron/network-service.ts')
  const service=new NetworkService(async()=>{}),calls=[]
  let closed=false,registered=true
  service.state.enabled[0]=true
  service.links[0]={epoch:0,tx:[],rx:[],tap:{},scanner:{stop(){}},
    stack:{interfaces:{remove:async()=>{assert(!closed,'Unregister before closing a tap stream');registered=false;calls.push('remove')}}},
    reader:{cancel:async()=>{assert(!registered);closed=true;calls.push('cancel')}},
    writer:{close:async()=>calls.push('close')}}
  await service.enable(0,false)
  assert.deepEqual(calls,['remove','cancel','close'])
  assert.equal(service.links[0],undefined)
  console.log('PASS tap shutdown ordering: unregister before reader cancellation, no closed-controller callback window')
}
async function testSessions() {
  const { NetworkSession } = load('kbm/src/services/network-session.ts')
  const remote = [], local = []
  const session = new NetworkSession(async (p,on) => remote.push([p,on]), async (p,on) => local.push([p,on]))
  await session.observe(0, { available: true, requested: false, ready: false })
  assert.equal(remote.length, 0, 'OFF starts OFF')
  await session.setDesired(0, true)
  await session.observe(0, { available: true, requested: false, ready: false })
  assert.deepEqual(remote, [[0,true]]); assert.equal(local.length,0, 'hidden NCM before DHCP')
  await session.observe(0, { available: true, requested: true, ready: true })
  assert.deepEqual(local, [[0,true]])
  await session.observe(0, { available: false, requested: false, ready: false })
  assert.deepEqual(local.at(-1), [0,false], 'disconnect destroys lease scope')
  await session.observe(0, { available: true, requested: false, ready: false })
  assert.deepEqual(remote.at(-1), [0,true])
  await session.observe(0, { available: true, requested: true, ready: true })
  assert.deepEqual(local.at(-1), [0,true])
  await session.suspend(); assert.deepEqual(session.desired,[false,false])
  await session.observe(0, { available: true, requested: true, ready: true })
  assert.deepEqual(local.at(-1), [0,false])
  session.suspended=false
  await session.observe(0, { available: true, requested: true, ready: true })
  assert.deepEqual(local.at(-1), [0,false], 'leaving native mode never auto-starts app services')
  console.log('PASS network desired/actual sessions, hidden NCM sequencing, reconnect lease reset, native exclusivity')
}
async function freePort() {
  const server = net.createServer()
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const port = server.address().port
  await new Promise(resolve => server.close(resolve))
  return port
}
async function testForwarding() {
  const { NetworkService } = load('kbm/src-electron/network-service.ts')
  const peers = [], taps = [], writers = []
  const service = new NetworkService(async (port, bytes) => { await writers[port].write(bytes) })
  for (const port of [0, 1]) {
    const stack = await tcpip.createStack({ initializeLoopback: false })
    const tap = await stack.interfaces.createTap({ ip: `192.168.127.${100 + port}/24`, mac: `02:00:00:00:00:0${port + 2}` })
    peers.push(stack); taps.push(tap); writers.push(tap.writable.getWriter())
    await service.enable(port, true)
    void (async () => { for await (const frame of tap) service.receive(port, Array.from(frame)) })()
    const listener = await stack.tcp.listen({ port: 22 })
    void (async () => {
      for await (const connection of listener) {
        void (async () => {
          const writer = connection.writable.getWriter()
          let first=true
          try { for await (const chunk of connection) { await writer.write(first?Uint8Array.of(port,...chunk):chunk);first=false } }
          finally { await writer.close().catch(() => {}) }
        })().catch(() => {})
      }
    })()
    const udp = await stack.udp.open({ port: 1234 })
    const writer = udp.writable.getWriter()
    void (async () => {
      for await (const packet of udp) await writer.write({ ...packet, data: Uint8Array.of(port, ...packet.data) })
    })().catch(() => {})
  }
  const tcp0 = await freePort(), tcp1 = await freePort(), udpPort = await freePort()
  await service.startForwarding([
    { id: 'a', port: 0, protocol: 'tcp', listenPort: tcp0, targetIp: '192.168.127.100', targetPort: 22 },
    { id: 'b', port: 1, protocol: 'tcp', listenPort: tcp1, targetIp: '192.168.127.101', targetPort: 22 },
    { id: 'c', port: 1, protocol: 'udp', listenPort: udpPort, targetIp: '192.168.127.101', targetPort: 1234 }
  ])
  await Promise.all([tcp0, tcp1].map((port, index) => timeout(new Promise((resolve, reject) => {
    const client = net.connect(port, '127.0.0.1', () => client.write(Buffer.from([2, 3, 27, 255])))
    const chunks = []
    client.on('data', chunk => {
      chunks.push(chunk)
      const all = Buffer.concat(chunks)
      if (all.length >= 5) { assert.deepEqual([...all], [index, 2, 3, 27, 255]); client.end(); resolve() }
    })
    client.on('error', reject)
  }))))
  // Real NetworkService + real lwIP peers, larger than the5792-byte TCP window.
  const binary=Buffer.from(Array.from({length:32768},(_,i)=>i*53&255))
  await Promise.all([tcp0,tcp1].map((port,index)=>timeout(new Promise((resolve,reject)=>{
    const client=net.connect(port,'127.0.0.1',()=>client.write(binary)),chunks=[]
    let total=0
    client.on('error',reject)
    client.on('data',chunk=>{chunks.push(chunk);total+=chunk.length;if(total>=binary.length+1){
      const actual=Buffer.concat(chunks);assert.equal(actual[0],index);assert.deepEqual(actual.subarray(1),binary);client.end();resolve()
    }})
  }))))
  await timeout(new Promise((resolve, reject) => {
    const socket = dgram.createSocket('udp4')
    socket.on('error', reject)
    socket.once('message', data => { assert.deepEqual([...data], [1, 9, 8, 7]); socket.close(); resolve() })
    socket.send(Buffer.from([9, 8, 7]), udpPort, '127.0.0.1')
  }))
  const link = service.links[0]
  let unblock
  service.send = () => new Promise(resolve => { unblock = resolve })
  for (let i = 0; i < 100; ++i) service.enqueue(0, link, new Uint8Array(60))
  assert(service.snapshot().queued[0] <= 32); assert(service.snapshot().dropped[0] > 0)
  await service.enable(0,false); unblock?.()
  assert(service.snapshot().enabled[1] && service.snapshot().forwarding, 'UART0 loss preserves UART1 listeners')
  assert(service.handles.every(handle => handle.port === 1))
  service.stopForwarding(); await service.stop()
  assert.equal(service.snapshot().forwarding, false)
  assert.deepEqual(service.snapshot().enabled, [false, false])
  console.log('PASS real lwIP TCP/UDP loopback → two isolated slave peers, binary transparency, bounded queues, stop cleanup')
}
;(async () => {
  const { ensureWasmStreaming } = load('kbm/src-electron/wasm-runtime.ts')
  const nativeStreaming = WebAssembly.instantiateStreaming
  ensureWasmStreaming()
  assert.equal(WebAssembly.instantiateStreaming, nativeStreaming, 'Keep a native implementation unchanged')
  WebAssembly.instantiateStreaming = undefined // Reproduce Electron main's API surface.
  ensureWasmStreaming()
  const fallback = WebAssembly.instantiateStreaming
  ensureWasmStreaming()
  assert.equal(WebAssembly.instantiateStreaming, fallback, 'Fallback installation is idempotent')
  const emptyModule = Uint8Array.of(0, 97, 115, 109, 1, 0, 0, 0)
  const compiled = await WebAssembly.instantiateStreaming(Promise.resolve(new Response(emptyModule)))
  assert(compiled.instance instanceof WebAssembly.Instance)
  await assert.rejects(WebAssembly.instantiateStreaming(new Response(Uint8Array.of(0))))
  console.log('PASS Electron missing instantiateStreaming fallback, native preservation, invalid WASM rejection')
  // Run the REAL lwIP + forwarding suite with streaming still unavailable natively.
  tcpip = await import(pathToFileURL(path.join(root, 'kbm/src-electron/node_modules/tcpip/dist/index.js')))
  testEthernet(); testDhcp(); await testMaster(); await testNativeEthernet(); await testShutdownOrdering(); await testSessions(); await testForwarding()
  console.log('All network tests passed (no hardware input).'); process.exit(0)
})().catch(error => { console.error(error); process.exit(1) })
