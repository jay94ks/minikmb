// Real in-memory lwIP, local Node echo servers and synthetic scanner packets.
// Never enables board NCM, scans a real host or changes OS network settings.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict'), net = require('node:net'), dgram = require('node:dgram'), { createRequire } = require('node:module'), { pathToFileURL } = require('node:url')
const root = path.resolve(__dirname, '..'), ts = require('../kbm/node_modules/typescript'), cache = new Map()
let tcpip
function load(relative) {
  const file = path.resolve(root, relative)
  if (cache.has(file)) return cache.get(file)
  const exports = {}, local = createRequire(file); cache.set(file, exports)
  const js = ts.transpileModule(fs.readFileSync(file, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
  vm.runInThisContext(`(function(exports,require){${js}\n})`, { filename: file })(exports, n => n === 'tcpip' ? tcpip : n.startsWith('.') ? load(path.relative(root, path.resolve(path.dirname(file), n + '.ts'))) : local(n)); return exports
}
const wait = ms => new Promise(r => setTimeout(r, ms))
async function timeout(promise, ms = 8000) { let timer; try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(Error('test timeout')), ms) })]) } finally { clearTimeout(timer) } }
const mac = s => Uint8Array.from(s.split(':').map(x => parseInt(x, 16)))
function arp(request, sourceMac, sourceIp) {
  const f = new Uint8Array(42), v = new DataView(f.buffer)
  f.set(request.subarray(6, 12)); f.set(sourceMac, 6); v.setUint16(12, 0x806); v.setUint16(14, 1); v.setUint16(16, 0x800); f[18] = 6; f[19] = 4; v.setUint16(20, 2)
  f.set(sourceMac, 22); f.set(Uint8Array.from(sourceIp.split('.').map(Number)), 28); f.set(request.subarray(22, 28), 32); f.set(request.subarray(28, 32), 38); return f
}
async function scanTest() {
  const { PortScanner } = load('kbm/src-electron/port-scanner.ts'), { tcpFrame, packet } = load('kbm/src-electron/ipv4-packets.ts')
  const server = mac('02:00:00:00:00:01'), client = mac('02:00:00:00:00:02'), frames = [], scanner = new PortScanner('192.168.127.1', server, f => frames.push(f))
  assert.throws(() => scanner.start({ target: '10.0.0.2', start: 1, end: 10, timeoutMs: 250 }))
  scanner.start({ target: '192.168.127.100', start: 22, end: 24, timeoutMs: 250 }); scanner.receive(arp(frames[0], client, '192.168.127.100')); await wait(0)
  const syns = frames.map(packet).filter(Boolean); assert.equal(syns.length, 3)
  for (const p of syns) {
    const seq = new DataView(p.frame.buffer).getUint32(38)
    if (p.destinationPort === 24) continue
    const reply = tcpFrame(client, server, '192.168.127.100', '192.168.127.1', p.destinationPort, p.sourcePort, 100, (seq + 1) >>> 0, p.destinationPort === 22 ? 0x12 : 0x14)
    assert(scanner.receive(reply))
  }
  await wait(300); const result = scanner.state(); assert(!result.running); assert.deepEqual(result.open, [22]); assert.equal(result.closed, 1); assert.equal(result.filtered, 1); assert.equal(result.completed, 3)
  assert(frames.some(f => f.length === 54 && f[47] === 0x14), 'open SYN must be cleaned up with RST')
  scanner.start({ target: '192.168.127.100', start: 1, end: 65535, timeoutMs: 250 }); scanner.stop(); await wait(0); assert(!scanner.state().running)
  console.log('PASS target-subnet TCP SYN scan, ARP resolution, open/closed/no-response, RST cleanup, bounds and cancellation; no real scan')
}
async function gatewayTest() {
  const { HostGateway } = load('kbm/src-electron/host-gateway.ts'), { tcpFrame, packet, validTransport, ipText } = load('kbm/src-electron/ipv4-packets.ts')
  load('kbm/src-electron/wasm-runtime.ts').ensureWasmStreaming()
  const nativeEvents=[]
  const nativeTcp = net.createServer(socket => { nativeEvents.push('server connected'); socket.on('data',b=>nativeEvents.push(['server received',b.length])); socket.on('end',()=>nativeEvents.push('server end')); socket.on('error',e=>nativeEvents.push(['server error',String(e)])); socket.pipe(socket) }), nativeUdp = dgram.createSocket('udp4')
  nativeUdp.on('message', (bytes, remote) => { nativeEvents.push(['udp server received',bytes.length]); nativeUdp.send(bytes, remote.port, remote.address) })
  await new Promise(r => nativeTcp.listen(0, '127.0.0.1', r)); await new Promise(r => nativeUdp.bind(0, '127.0.0.1', r))
  const hostStack = await tcpip.createStack({ initializeLoopback: false }), peerStack = await tcpip.createStack({ initializeLoopback: false })
  const hostTap = await hostStack.interfaces.createTap({ ip: '192.168.127.1/24', mac: '02:00:00:00:00:01' }), peerTap = await peerStack.interfaces.createTap({ ip: '192.168.127.100/0', mac: '02:00:00:00:00:02' })
  const hostWriter = hostTap.writable.getWriter(), peerWriter = peerTap.writable.getWriter(), hostReader = hostTap.readable.getReader(), peerReader = peerTap.readable.getReader(), errors = [], dialed = []
  let stopped = false, listenerCount = 0, dropped = 0, phase = 'setup'; const trace = []
  const listen = hostStack.tcp.listen; hostStack.tcp.listen = options => { listenerCount++; return listen(options) }
  const gateway = new HostGateway(hostStack, () => true, () => dropped++, {
    tcp: (host, port) => { dialed.push(['tcp', host, port]); const s=net.createConnection({ host: '127.0.0.1', port: nativeTcp.address().port, allowHalfOpen: true });s.on('connect',()=>nativeEvents.push('client connect'));s.on('end',()=>nativeEvents.push('client end'));s.on('error',e=>nativeEvents.push(['client error',String(e)]));s.on('close',()=>nativeEvents.push('client close')); return s },
    udp: (host, port) => { dialed.push(['udp', host, port]); const socket = dgram.createSocket('udp4');socket.on('error',e=>nativeEvents.push(['udp error',String(e)]));socket.on('connect',()=>nativeEvents.push('udp connected'));socket.on('message',b=>nativeEvents.push(['udp client received',b.length])); socket.connect(nativeUdp.address().port, '127.0.0.1'); return socket }, dns: () => '203.0.113.53'
  }, e => errors.push(e))
  await gateway.start('192.168.127.1')
  const loops = [
    (async () => { try { while (!stopped) { const d = await hostReader.read(); if (d.done) break; const frame = gateway.egress(d.value); trace.push(['host',packet(d.value),Boolean(frame)]); if (trace.length>15)trace.shift(); if (frame) { const p = packet(frame); if (p) assert(validTransport(p)); await peerWriter.write(frame) } } } catch (e) { if (!stopped) errors.push(e) } })(),
    (async () => { try { while (!stopped) { const d = await peerReader.read(); if (d.done) break; const f = d.value
      // Model the target OS's next-hop neighbor. tcpip's test peer has no route API.
      if (f[12] === 8 && f[13] === 6 && f[21] === 1) { await peerWriter.write(arp(f, mac(hostTap.mac), ipText(f.subarray(38, 42)))); continue }
      const frame = await gateway.ingress(f); if (frame) await hostWriter.write(frame)
      trace.push(['peer',packet(f),Boolean(frame)]); if(trace.length>15)trace.shift()
    } } catch (e) { if (!stopped) errors.push(e) } })()
  ]
  let tcpPeer, udpPeer
  try {
    const blocked = tcpFrame(mac(peerTap.mac), mac(hostTap.mac), '192.168.127.100', '127.0.0.1', 50000, 22, 1, 0, 2)
    assert.equal(await gateway.ingress(blocked), undefined); assert.equal(dialed.length, 0)
    phase = 'tcp-connect'; tcpPeer = await timeout(peerStack.tcp.connect({ host: '198.51.100.10', port: 443 }))
    const writer = tcpPeer.writable.getWriter(), reader = tcpPeer.readable.getReader(), data = Uint8Array.from({ length: 16384 }, (_, i) => i * 17 & 255)
    const read = (async () => { const out = new Uint8Array(data.length); let at = 0; while (at < out.length) { const d = await reader.read(); assert(!d.done); out.set(d.value, at); at += d.value.length }; assert.deepEqual(out, data) })()
    phase = 'tcp-write'; for(let at=0;at<data.length;at+=4096) {phase=`tcp-write:${at}`;await timeout(writer.write(data.subarray(at,at+4096)))}; phase = 'tcp-read'; await timeout(read); reader.releaseLock(); writer.releaseLock(); await tcpPeer.close(); tcpPeer = undefined
    assert(dialed.some(d => d[0] === 'tcp' && d[1] === '198.51.100.10' && d[2] === 443))
    phase = 'udp'; udpPeer = await peerStack.udp.open({ host: '192.168.127.100', port: 45000 })
    const uw = udpPeer.writable.getWriter(), ur = udpPeer.readable.getReader()
    await uw.write({ host: '198.51.100.11', port: 12345, data: Uint8Array.of(1, 2, 3, 4) }); let reply = await timeout(ur.read()); assert.equal(reply.value.host, '198.51.100.11'); assert.equal(reply.value.port, 12345); assert.deepEqual(reply.value.data, Uint8Array.of(1, 2, 3, 4))
    await uw.write({ host: '192.168.127.1', port: 53, data: Uint8Array.of(9, 8, 7) }); reply = await timeout(ur.read()); assert.equal(reply.value.host, '192.168.127.1'); assert.equal(reply.value.port, 53)
    assert(dialed.some(d => d[1] === '203.0.113.53' && d[2] === 53)); uw.releaseLock(); ur.releaseLock(); await udpPeer.close(); udpPeer = undefined
    gateway.stop(); assert.equal(gateway.count, 0); await gateway.start('192.168.127.1'); gateway.stop(); await gateway.start('192.168.127.1'); assert.equal(listenerCount, 8, 'reuse fixed listeners across mode changes')
    assert.equal(errors.length, 0, errors.map(String).join('\n'))
    console.log('PASS real lwIP→local native TCP/UDP round trips, checksum/address/port translation, DNS relay, loopback rejection, shutdown and fixed listener reuse; no external connection')
  } catch(e) { console.error(JSON.stringify({ phase,dropped,dialed,nativeEvents,errors:errors.map(String),trace:trace.map(([dir,p,ok])=>[dir,p&&{source:p.source,destination:p.destination,sourcePort:p.sourcePort,destinationPort:p.destinationPort,flags:p.frame[p.offset+13],length:p.frame.length},ok]) },null,2)); throw e } finally {
    stopped = true; gateway.stop(); await tcpPeer?.close().catch(() => {}); await udpPeer?.close().catch(() => {})
    await hostStack.interfaces.remove(hostTap); await peerStack.interfaces.remove(peerTap); await hostReader.cancel(); await peerReader.cancel(); await hostWriter.close(); await peerWriter.close(); await Promise.all(loops)
    nativeTcp.close(); nativeUdp.close()
  }
}
;(async () => { tcpip = await import(pathToFileURL(path.join(root, 'kbm/src-electron/node_modules/tcpip/dist/index.js'))); await scanTest(); await gatewayTest(); process.exit(0) })().catch(e => { console.error(e); process.exit(1) })
