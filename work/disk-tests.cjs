// Pure filesystem fixtures + mock packets. Never opens a physical disk/USB/UART.
const fs = require('node:fs/promises'), path = require('node:path'), vm = require('node:vm')
const assert = require('node:assert/strict'), { createRequire } = require('node:module')
const root = path.resolve(__dirname, '..'), ts = require(path.join(root, 'kbm/node_modules/typescript'))
const modules = new Map()
function load(relative) {
  const file = path.resolve(root, relative)
  if (modules.has(file)) return modules.get(file)
  const exports = {}; modules.set(file, exports)
  const req = createRequire(file)
  const code = ts.transpileModule(require('node:fs').readFileSync(file, 'utf8'), { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true
  } }).outputText
  vm.runInThisContext(`(function(exports,require){${code}\n})`, { filename: file })(exports,
    name => name.startsWith('.') ? load(path.relative(root, path.resolve(path.dirname(file), name + '.ts'))) : req(name))
  return exports
}
function directory(bytes) {
  const entries = []; let long = []
  for (let at = 0; at < bytes.length; at += 32) {
    const e = bytes.subarray(at, at + 32), v = new DataView(e.buffer, e.byteOffset, 32)
    if (!e[0]) break
    if (e[11] === 15) {
      const units = [1,3,5,7,9,14,16,18,20,22,24,28,30].map(n => v.getUint16(n,true))
      long[e[0] & 31] = units; continue
    }
    const name = long.length ? String.fromCharCode(...long.slice(1).flat().filter(n => n !== 0 && n !== 65535)) : Buffer.from(e.subarray(0,11)).toString().trim()
    long = []
    if (!(e[11] & 8)) entries.push({ name, directory: Boolean(e[11] & 16), cluster: v.getUint16(26,true)+v.getUint16(20,true)*65536, size: v.getUint32(28,true) })
  }
  return entries
}
async function run() {
  const { FolderDisk } = load('kbm/src-electron/folder-disk.ts')
  const { DiskService } = load('kbm/src-electron/disk-service.ts')
  const temp = await fs.mkdtemp(path.join(root, 'work/disk-fixture-'))
  assert(temp.startsWith(path.join(root, 'work/disk-fixture-')))
  try {
    const shared = path.join(temp, 'share'); await fs.mkdir(shared)
    const binary = Buffer.from(Array.from({ length: 9000 }, (_, i) => i * 53 & 255))
    await fs.writeFile(path.join(shared, '한글 긴 이름😀.bin'), binary)
    await fs.writeFile(path.join(shared, 'empty.txt'), '')
    await fs.mkdir(path.join(shared, 'sub')); await fs.writeFile(path.join(shared, 'sub/note.txt'), 'USB folder disk\n')
    const external = path.join(temp, 'outside'); await fs.mkdir(external); await fs.writeFile(path.join(external,'secret.txt'),'NOT SHARED')
    let links = false
    try { await fs.symlink(external,path.join(shared,'outside-link'),'junction'); links = true } catch (error) { if(error.code !== 'EPERM') throw error }
    const disk = await FolderDisk.create(shared), boot = await disk.readSector(0), view = new DataView(boot.buffer)
    assert.equal(view.getUint16(11,true),512); assert.equal(boot[13],8); assert.equal(boot[16],2)
    assert.equal(view.getUint16(510,true),0xaa55); assert.equal(view.getUint32(44,true),2)
    assert.equal(disk.info.files,3); assert.equal(disk.info.bytes,9016)
    assert.deepEqual(await disk.readSector(6),boot)
    const damaged=await disk.readSector(0);damaged[0]=0
    assert.equal((await disk.readSector(0))[0],0xeb,'Metadata cache must not expose mutable buffers')
    const fatSize = view.getUint32(36,true), start = 32+fatSize*2
    const fsinfo = new DataView((await disk.readSector(1)).buffer)
    assert.equal(fsinfo.getUint32(0,true),0x41615252); assert.equal(fsinfo.getUint32(508,true),0xaa550000)
    const firstFat = await disk.readSector(32); assert.deepEqual(await disk.readSector(32+fatSize),firstFat)
    const fat = async cluster => new DataView((await disk.readSector(32+Math.floor(cluster/128))).buffer).getUint32(cluster%128*4,true)&0x0fffffff
    const readChain = async cluster => {
      const blocks=[]; let hops=0
      while(cluster<0x0ffffff8) {
        assert(++hops<100)
        for(let n=0;n<8;n++) blocks.push(await disk.readSector(start+(cluster-2)*8+n))
        cluster = await fat(cluster)
      }
      return Buffer.concat(blocks)
    }
    const listing = directory(await readChain(2))
    assert.deepEqual(listing.map(e=>e.name).sort(),['empty.txt','sub','한글 긴 이름😀.bin'].sort())
    const file = listing.find(e=>e.name.endsWith('.bin'))
    assert.equal(file.size,binary.length); assert.deepEqual((await readChain(file.cluster)).subarray(0,file.size),binary)
    for(let sectors=1;sectors<=8;sectors++) {
      const lba=start+(file.cluster-2)*8,expected=[]
      for(let n=0;n<sectors;n++)expected.push(await disk.readSector(lba+n))
      assert.deepEqual(Buffer.from(await disk.readSectors(lba,sectors)),Buffer.concat(expected),'Batched file range matches individual sectors including tail padding')
    }
    const empty = listing.find(e=>e.name==='empty.txt'); assert.equal(empty.cluster,0)
    const sub = listing.find(e=>e.name==='sub'), nested = directory(await readChain(sub.cluster))
    assert(nested.some(e=>e.name==='.' && e.cluster===sub.cluster)); assert(nested.some(e=>e.name==='..' && e.cluster===0))
    const note = nested.find(e=>e.name==='note.txt'); assert.equal((await readChain(note.cluster)).subarray(0,note.size).toString(),'USB folder disk\n')
    await assert.rejects(disk.readSector(-1)); await assert.rejects(disk.readSector(disk.info.sectors))
    await fs.writeFile(path.join(shared,'new.txt'),'not visible until refresh')
    assert(!directory(await readChain(2)).some(e=>e.name==='new.txt'))
    await fs.writeFile(path.join(shared,'한글 긴 이름😀.bin'),Buffer.from('changed'))
    await assert.rejects(disk.readSector(start+(file.cluster-2)*8),/변경/)
    await assert.rejects(disk.readSectors(start+(file.cluster-2)*8,8),/변경/)
    for(let n=0;n<600;n++) await disk.readSector(32+n)
    assert(disk.metadataCache.size<=512,'Metadata cache stays bounded')
    const service = new DiskService(), info = await service.prepare(0,shared)
    assert.equal(service.snapshot().snapshots[1],null)
    assert.equal((await service.read(0,{generation:info.generation,id:1,lba:0})).length,512)
    const expectedSectors = []
    for (let lba=0;lba<8;lba++) expectedSectors.push(await service.read(0,{generation:info.generation,id:2,lba}))
    for (let sectors=1;sectors<=8;sectors++) {
      const actual = await service.read(0,{generation:info.generation,id:3,lba:0,sectors})
      assert.equal(actual.length,sectors*512)
      assert.deepEqual(Buffer.from(actual),Buffer.concat(expectedSectors.slice(0,sectors)))
    }
    for (const sectors of [0,9,-1,1.5,NaN]) await assert.rejects(service.read(0,{generation:info.generation,id:4,lba:0,sectors}))
    await assert.rejects(service.read(0,{generation:info.generation,id:4,lba:info.sectors-1,sectors:2}),/outside/)
    await testRealServiceTransfer(service,info)
    await assert.rejects(service.read(0,{generation:info.generation^1,id:1,lba:0}))
    service.disable(0); await assert.rejects(service.read(0,{generation:info.generation,id:1,lba:0}))
    assert.equal((await FolderDisk.create(shared)).info.files,4)
    console.log(`PASS FAT32 boot/backup/FSInfo/mirrored FAT/chains, Korean/emoji LFN, directories, binary/empty files, manifest refresh, mutation detection, per-port generation${links?', junction exclusion':''}`)
  } finally { await fs.rm(temp,{recursive:true,force:true}) }
  await testWire()
  await testSession()
  await testNativeTransport()
  await testOperationLog()
  testFrames16()
  console.log('All virtual disk tests passed (no physical disk/HID access).')
}
async function testOperationLog() {
  const {OperationLogger}=load('kbm/src-electron/operation-log.ts')
  const temp=await fs.mkdtemp(path.join(root,'work/log-fixture-'))
  try {
    const logs=new OperationLogger(path.join(temp,'logs'),1024,3)
    logs.log('info','privacy.test',{port:0,payload:'SECRET_PACKET',keycode:42,filename:'SECRET_NAME',error:'ENOENT C:\\private\\secret.txt',note:'line1\nline2'})
    logs.log('debug','hidden.debug')
    await Promise.all([logs.flush(),logs.flush()])
    const content=await fs.readFile(logs.file,'utf8'),entry=JSON.parse(content.trim())
    assert(!content.includes('SECRET'));assert(!content.includes('private'))
    assert.equal(entry.fields.note,'line1 line2');assert.equal(entry.fields.port,0)
    assert(!content.includes('hidden.debug'))
    await logs.configure(false,false);logs.log('error','disabled.entry');await logs.flush()
    assert(!logs.snapshot().recent.some(e=>e.event==='disabled.entry'))
    await logs.configure(true,true);logs.log('debug','visible.debug')
    await logs.flush();assert(logs.snapshot().recent.some(e=>e.event==='visible.debug'))
    for(let n=0;n<1100;n++)logs.log('info','rotation.test',{n,note:'x'.repeat(100)})
    await logs.flush()
    assert.equal(logs.snapshot().recent.length,300);assert(logs.snapshot().dropped>0)
    const files=await fs.readdir(logs.directory);assert(files.length<=3)
    for(const file of files){const bytes=await fs.readFile(path.join(logs.directory,file));assert(bytes.length<=1024);for(const line of bytes.toString().trim().split('\n'))JSON.parse(line)}
    const blocked=path.join(temp,'not-a-directory');await fs.writeFile(blocked,'fixture')
    const failed=new OperationLogger(blocked);failed.log('error','test.error');await failed.flush()
    assert(failed.snapshot().writeError);assert.equal(failed.snapshot().dropped,1)
    console.log('PASS operation logging: JSONL, privacy, ON/OFF/debug, bounded300/1024 buffers, rotation limits, concurrent flush and nonfatal write failure')
  }finally{await fs.rm(temp,{recursive:true,force:true})}
}
async function testRealServiceTransfer(service,info) {
  const { RoutedTransport }=load('kbm/src-electron/routed-transport.ts')
  const { DiskTransfer }=load('kbm/src-electron/disk-transfer.ts')
  const { ProgressiveDecoder }=load('kbm/src/services/protocol.ts')
  const { crc32 }=load('kbm/src/services/ethernet.ts')
  const decoder=new ProgressiveDecoder(),packets=[]
  const transport=new RoutedTransport(async bytes=>{
    for(const event of decoder.feed(bytes)) if(event.type==='packet') {
      const p=event.packet;packets.push(p)
      queueMicrotask(()=>transport.accept({op:0xf3,payload:Uint8Array.of(p.payload[0],p.payload[1],p.payload[2],0,p.payload[3]|128,0,0)}))
    }
  })
  const transfer=new DiskTransfer(service,transport)
  let expectedCount=0
  for(const sectors of [1,2,3,4,5,6,7,8]) {
    const token={generation:info.generation,id:10+sectors,lba:0,sectors}
    const expected=await service.read(0,token)
    await transfer.serve(0,token)
    const p=packets.at(-1)
    assert.equal(p.payload[3],0x45,'Real DiskService must produce DATA, not FAIL')
    const data=p.payload.subarray(4),view=new DataView(data.buffer,data.byteOffset,data.length)
    assert.equal(data.length,18+512*sectors)
    assert.equal(view.getUint32(14,true),crc32(expected))
    assert.deepEqual(data.subarray(18),expected)
    expectedCount+=sectors
  }
  assert.equal(transfer.snapshot()[0].sectors,expectedCount)
  assert.equal(transfer.snapshot()[0].errors,0)
  assert.equal(packets.length,8)
  console.log('PASS real DiskService -> DiskTransfer -> encoded ROUTE integration: every1..8-sector count, exact bytes and CRC, no FAIL')
}
function testFrames16() {
  const { ProgressiveDecoder, encodePacket }=load('kbm/src/services/protocol.ts')
  for(const n of [0,1,255,256,530,4114,4118]) {
    const p={op:0x45,payload:Uint8Array.from({length:n},(_,i)=>[2,3,27,0,255][i%5])}
    const wire=encodePacket(p)
    assert.equal(wire[2],n&255);assert.equal(wire[3],n>>>8)
    let sum=0x45+(n&255)+(n>>>8);for(const b of p.payload)sum+=b
    assert.equal(wire.at(-1),sum&255)
    for(const chunk of [1,2,63,512,4096]) {
      const decoder=new ProgressiveDecoder(),events=[]
      for(let at=0;at<wire.length;at+=chunk) events.push(...decoder.feed(wire.subarray(at,at+chunk)))
      assert.equal(events.length,1);assert.deepEqual(events[0],{type:'packet',packet:p})
    }
  }
  assert.equal(encodePacket({op:0,payload:new Uint8Array()}).length,6,'PING is not padded to4KiB')
  assert.throws(()=>encodePacket({op:0x45,payload:new Uint8Array(4119)}))
  const decoder=new ProgressiveDecoder()
  assert.equal(decoder.feed(Uint8Array.of(2,0x45,255,255))[0].error,'length')
  assert.equal(decoder.feed(encodePacket({op:0,payload:new Uint8Array()}))[0].type,'packet')
  console.log('PASS v3 LEN16 LE, short/unpadded control, maximum4KiB FS envelope, every chunk boundary, length overflow and recovery')
}
async function testNativeTransport() {
  const { RoutedTransport }=load('kbm/src-electron/routed-transport.ts')
  const { DiskTransfer }=load('kbm/src-electron/disk-transfer.ts')
  const { ProgressiveDecoder }=load('kbm/src/services/protocol.ts')
  const { crc32 }=load('kbm/src/services/ethernet.ts')
  const decoder=new ProgressiveDecoder(), writes=[]
  const transport=new RoutedTransport(async bytes=>{ for(const item of decoder.feed(bytes)) if(item.type==='packet') writes.push(item.packet) })
  const frame=(port,id,op=0x45)=>({op:0x73,payload:Uint8Array.of(port,id&255,id>>>8,op)})
  const ack=(p,status=0)=>transport.accept({op:0xf3,payload:Uint8Array.of(p.payload[0],p.payload[1],p.payload[2],0,p.payload[3]|128,status,0)})
  const a=transport.request(frame(0,65535),true), first=writes[0]
  const b=transport.request(frame(0,0),true), hid=transport.request(frame(0,65535,0x10))
  const other=transport.request(frame(1,65535,0))
  assert.equal(writes.length,2,'Independent UART progresses during UART0 wait')
  ack(writes[1]); await other
  assert(!transport.accept({op:0xf8,payload:new Uint8Array(13)}),'Async event is never an ACK')
  ack(first); const restored=await a
  assert.equal(restored.payload[1],255);assert.equal(restored.payload[2],255,'Renderer request ID restored')
  await Promise.resolve();assert.equal(writes[2].payload[3],0x10,'HID precedes queued disk fragment')
  ack(first);assert(transport.active[0],'Stale ACK cannot consume HID')
  ack(writes[2]);await hid;await Promise.resolve();ack(writes[3]);await b
  let current=true
  const hold=transport.request(frame(0,1,0)), canceled=transport.request(frame(0,2),true,()=>current)
  const canceledCheck=assert.rejects(canceled,/Canceled/);current=false
  ack(writes.at(-1));await hold;await canceledCheck
  const pending=transport.request(frame(0,3)), queued=transport.request(frame(0,4),true)
  const checks=[assert.rejects(pending,/disconnected/),assert.rejects(queued,/disconnected/)]
  transport.reset();await Promise.all(checks)
  console.log('PASS native UART arbiter: independent ports, HID/control priority, true ACK stop-and-wait, ID remap, stale ACK, cancellation and reset')

  let revision=1, tick=0, auto, cancelFirst=false, rejectData=false
  const packets=[], wireDecoder=new ProgressiveDecoder(), data=Uint8Array.from({length:4096},(_,i)=>i*53&255)
  const disks={revision:()=>revision,isCurrent:(_port,generation)=>generation===42,
    read:async()=>{tick+=1;return data}}
  auto=new RoutedTransport(async bytes=>{
    for(const item of wireDecoder.feed(bytes)) if(item.type==='packet') {
      const p=item.packet;packets.push(p);tick+=2
      if(cancelFirst&&p.payload[3]===0x45) {revision++;cancelFirst=false}
      queueMicrotask(()=>auto.accept({op:0xf3,payload:Uint8Array.of(p.payload[0],p.payload[1],p.payload[2],0,p.payload[3]|128,p.payload[3]===0x45&&rejectData?3:0,0)}))
    }
  })
  const server=new DiskTransfer(disks,auto,()=>tick), token={generation:42,id:3,lba:9,sectors:8}
  const serve=server.serve(0,token);await server.serve(0,token);await serve
  const fragments=packets.filter(p=>p.payload[3]===0x45)
  assert.equal(fragments.length,1)
  for(let i=0;i<1;i++) {
    const payload=fragments[i].payload.subarray(4),v=new DataView(payload.buffer,payload.byteOffset,payload.length)
    assert.equal(v.getUint16(12,true),i*64);assert.equal(v.getUint32(14,true),crc32(data))
    assert.deepEqual(payload.subarray(18),data)
  }
  assert.equal(server.snapshot()[0].sectors,8);assert.equal(server.snapshot()[0].duplicates,1)
  assert.equal(server.snapshot()[0].sourceMs,1);assert.equal(server.snapshot()[0].transferMs,2)
  const count=packets.length;cancelFirst=true;await server.serve(0,{...token,id:4})
  assert.equal(packets.length-count,1,'No next fragment after share OFF/refresh')
  rejectData=true;await server.serve(1,token)
  assert.equal(server.snapshot()[1].errors,1);assert.equal(packets.at(-1).payload[3],0x47)
  assert(!packets.some(p=>[0x10,0x11,0x20,0x21,0x22,0x60].includes(p.payload[3])),'Native disk producer never sends HID/reboot')
  console.log('PASS main-only4096-byte variable CRC transfer, no renderer IPC, real ACK gates, timing metrics, duplicate F8, revision cancellation and FAIL')
}
async function testWire() {
  const { MasterSerialClient } = load('kbm/src/services/master-serial.ts')
  const { ProgressiveDecoder, encodePacket } = load('kbm/src/services/protocol.ts')
  const { crc32 } = load('kbm/src/services/ethernet.ts')
  const decoder = new ProgressiveDecoder(), writes=[]; let deliver, diskFlags=3
  global.window={kbmNative:{master:{ connect:async()=>true,onData:listener=>{deliver=listener},onConnection:()=>{},onError:()=>{},write:async bytes=>{
    for(const event of decoder.feed(bytes)) if(event.type==='packet') {
      const p=event.packet; writes.push(p)
      if(p.op===0x71) deliver(encodePacket({op:0xf1,payload:Uint8Array.from({length:21},(_,i)=>i===1?3:i===2?3:0)}))
      else if(p.op===0x77) deliver(encodePacket({op:0xf7,payload:Uint8Array.of(0,1,0,1,0)})) // Native NCM does not block disk serving.
      else if(p.op===0x73) {
        const reply = p.payload[3]===0x46 ? [0,diskFlags] : p.payload[3]===0x41 ? [0,3,2,0,0,0,0,1,0,194,1,0] : [0,0]
        deliver(encodePacket({op:0xf3,payload:Uint8Array.of(p.payload[0],p.payload[1],p.payload[2],0,p.payload[3]|128,...reply)}))
      }
    }
  }}}}
  const client=new MasterSerialClient(); assert(await client.initialize())
  let received
  client.onDiskRead((port,request)=>{ received={port,...request} })
  const payload=new Uint8Array(15),v=new DataView(payload.buffer); payload[0]=1; v.setUint32(1,123,true);v.setUint32(5,77,true);v.setUint32(9,42,true);v.setUint16(13,1,true)
  deliver(encodePacket({op:0xf8,payload})); assert.deepEqual(received,{port:1,generation:123,id:77,lba:42,sectors:1})
  await client.diskConfigure(1,123,1024,true)
  assert.deepEqual(await client.diskStatus(1),{configured:true,ready:true})
  const network=await client.networkStatus(1)
  assert.deepEqual(network,{enabled:true,ready:true,mac:'02:00:00:00:00:01',advertisedBps:115200})
  diskFlags=0; assert.deepEqual(await client.diskStatus(0),{configured:false,ready:false})
  diskFlags=8; await assert.rejects(client.diskStatus(0),/상태/); diskFlags=3
  const countBefore=writes.length
  for(const generation of [0,-1,1.5,0x100000000]) await assert.rejects(client.diskConfigure(0,generation,1,true))
  await assert.rejects(client.diskConfigure(0,1,0,true)); await assert.rejects(client.diskConfigure(0,1,67108865,true))
  assert.equal(writes.length,countBefore,'Invalid disk config never reaches transport')
  const validReceived=received
  v.setUint32(5,0,true); deliver(encodePacket({op:0xf8,payload})); assert.equal(received,validReceived)
  const sector=Uint8Array.from({length:512},(_,i)=>i&255)
  await client.diskData(1,{generation:123,id:77,lba:42},sector,()=>true)
  const fragments=writes.filter(p=>p.op===0x73 && p.payload[3]===0x45)
  assert.equal(fragments.length,1)
  for(let i=0;i<1;i++) {
    const p=fragments[i].payload.subarray(4), view=new DataView(p.buffer,p.byteOffset,p.byteLength)
    assert.equal(view.getUint16(12,true),i*64); assert.equal(view.getUint32(14,true),crc32(sector))
    assert.deepEqual(p.subarray(18),sector)
  }
  await client.diskFail(1,{generation:123,id:77,lba:42}); client.handleDisconnected()
  console.log('PASS F8 sector-count tokens, C6/C1 status, bounds, native coexistence, short512-byte single DATA CRC, true ACK correlation')
}
async function testSession() {
  const { DiskSession } = load('kbm/src/services/disk-session.ts')
  let now=0, generation=10, holdRead, finishRead, holdPrepare, finishPrepare, configureFailure=false
  const remote=[{configured:false,ready:false},{configured:false,ready:false}], calls=[]
  const backend={
    prepare:async port=> { if(holdPrepare) await new Promise(resolve=>{finishPrepare=resolve}); return {generation:++generation,folder:'mock',files:1,bytes:512,sectors:1024} },
    disable:async port=>{calls.push(['disable',port])},
    configure:async (port,gen,sectors,enabled)=>{calls.push(['config',port,gen,enabled]); if(configureFailure&&enabled) throw new Error('lost CONFIG ACK')},
    status:async port=>({...remote[port]}),
    read:async()=> { if(holdRead) await new Promise(resolve=>{finishRead=resolve}); return new Uint8Array(512) },
    data:async (port,request,bytes,current)=>{assert(current());calls.push(['data',port,request.generation])},
    fail:async port=>{calls.push(['fail',port])}
  }
  const session=new DiskSession(backend,()=>{},()=>now)
  session.observe(0,true); session.observe(1,true)
  await Promise.all([session.tick(0),session.tick(1)])
  assert(calls.every(call=>call[0]!=='config'),'No auto ON at launch')
  await session.start(0); assert.equal(session.ports[0].phase,'starting','ACK is not readiness')
  remote[0]={configured:true,ready:true}; await session.tick(0); assert.equal(session.ports[0].phase,'active')
  await session.start(1); remote[1]={configured:true,ready:true}; await session.tick(1)
  const token={generation:session.ports[0].snapshot.generation,id:1,lba:0}
  await session.serve(0,{...token,generation:token.generation-1}); assert(!calls.some(c=>c[0]==='data'))
  await session.serve(0,token); assert.equal(session.ports[0].reads,1)
  now=2100; await Promise.all([session.tick(0),session.tick(1)])
  assert.equal(calls.filter(c=>c[0]==='config'&&c[3]).length,4,'Two starts plus two heartbeats')
  holdRead=true; const oldRead=session.serve(0,token); await new Promise(resolve=>setImmediate(resolve))
  session.observe(0,false); finishRead(); await oldRead; holdRead=false
  assert.equal(calls.filter(c=>c[0]==='data').length,1,'No stale read after disconnect')
  assert(session.ports[1].enabled,'Other UART continues')
  session.observe(0,true); await session.tick(0)
  assert(!session.ports[0].enabled,'Reconnect must not reactivate share')
  remote[0]={configured:false,ready:false}; await session.tick(0)
  await session.start(0); remote[0]={configured:true,ready:true}; await session.tick(0)
  const beforeOff=calls.filter(c=>c[0]==='config'&&c[3]).length
  await session.stop(0); remote[0]={configured:false,ready:false}; now+=3000; await session.tick(0)
  assert.equal(session.ports[0].phase,'hidden')
  assert.equal(calls.filter(c=>c[0]==='config'&&c[3]).length,beforeOff,'No heartbeat after OFF')
  await session.start(0); remote[0]={configured:true,ready:false}; now+=1100; await session.tick(0)
  assert.equal(session.ports[0].phase,'ejected'); assert(!session.ports[0].enabled)
  const oldGeneration=session.ports[0].snapshot.generation
  await session.start(0); assert(session.ports[0].snapshot.generation>oldGeneration)
  remote[0]={configured:true,ready:true}; await session.tick(0)
  await session.stop(0); configureFailure=true; await session.start(0)
  assert.equal(session.ports[0].phase,'error'); assert(!session.ports[0].enabled)
  const afterLost=calls.length; now+=3000; await session.tick(0)
  assert(!calls.slice(afterLost).some(c=>c[0]==='config'&&c[3]),'Lost ACK never retries ON')
  configureFailure=false; holdPrepare=true
  const start=session.start(0); await new Promise(resolve=>setImmediate(resolve))
  assert(finishPrepare); const stop=session.stop(0); finishPrepare(); await Promise.all([start,stop]); holdPrepare=false
  assert(!session.ports[0].enabled && !session.ports[0].busy)
  remote[0]={configured:false,ready:false}; await session.tick(0); assert.equal(session.ports[0].phase,'hidden')
  assert(session.ports[1].enabled)
  console.log('PASS disk sessions: ACK/readiness, dual UARTs, heartbeat, OFF, eject, manual refresh, lost ACK, canceled prepare/stale read, no automatic reconnect ON')
}
run().catch(error=>{console.error(error);process.exit(1)})
