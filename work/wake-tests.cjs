// Pure app mocks only. Never wakes a host, opens COM or sends LAN packets.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict')
const {createRequire}=require('node:module'),{EventEmitter}=require('node:events'),ts=require('../kbm/node_modules/typescript')
const root=path.resolve(__dirname,'..'),cache=new Map()
function load(relative){const file=path.resolve(root,relative);if(cache.has(file))return cache.get(file);const exports={};cache.set(file,exports);const local=createRequire(file)
  const code=ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText
  vm.runInThisContext(`(function(exports,require){${code}\n})`,{filename:file})(exports,name=>name.startsWith('.')?load(path.relative(root,path.resolve(path.dirname(file),name+'.ts'))):local(name));return exports}
;(async()=>{
  const{magicPacket,WakeOnLan}=load('kbm/src-electron/wake-on-lan.ts'),{normalizeWakeMac}=load('kbm/src/shared/wake.ts')
  assert.equal(normalizeWakeMac('00-11-22-33-44-55'),'00:11:22:33:44:55')
  for(const bad of['001122','01:11:22:33:44:55','ff:ff:ff:ff:ff:ff','00:00:00:00:00:00','zz001122334455'])assert.throws(()=>magicPacket(bad))
  const packet=magicPacket('00:11:22:33:44:55');assert.equal(packet.length,102);assert.deepEqual([...packet.slice(0,6)],Array(6).fill(255))
  for(let n=0;n<16;n++)assert.deepEqual([...packet.slice(6+n*6,12+n*6)],[0,17,34,51,68,85])
  const interfaces=()=>({LAN:[{internal:false,family:'IPv4',address:'192.168.10.2',netmask:'255.255.255.0'}],lo:[{internal:true,family:'IPv4',address:'127.0.0.1',netmask:'255.0.0.0'}]})
  const sent=[],logs=[];let closes=0,factories=0,failure
  const factory=()=>{factories++;if(failure==='factory')throw Object.assign(Error('unsupported'),{code:'ENOTSUP'});const socket=new EventEmitter()
    socket.bind=(port,ip,cb)=>{assert.equal(ip,'192.168.10.2');queueMicrotask(cb)}
    socket.setBroadcast=value=>assert.equal(value,true)
    socket.send=(bytes,port,address,cb)=>{sent.push({bytes,port,address});cb(failure?Object.assign(Error('send failure'),{code:failure}):null)}
    socket.close=()=>closes++;return socket}
  const service=new WakeOnLan((...args)=>logs.push(args),interfaces,factory),config={enabled:true,mac:'00:11:22:33:44:55',localAddress:'192.168.10.2'}
  assert.deepEqual(await service.send(0,{enabled:false}),{code:129});assert.equal(factories,0)
  assert.deepEqual(await service.send(1,config),{code:128});assert.equal(sent.length,1);assert.equal(sent[0].port,9);assert.equal(sent[0].address,'192.168.10.255');assert.deepEqual(sent[0].bytes,packet);assert.equal(closes,1)
  failure='ENOTSUP';assert.deepEqual(await service.send(0,config),{code:130});assert.equal(closes,2)
  failure='factory';assert.deepEqual(await service.send(0,config),{code:130})
  failure='EHOSTUNREACH';await assert.rejects(service.send(0,config),/send failure/)
  await assert.rejects(service.send(2,config));await assert.rejects(service.send(0,{...config,localAddress:'8.8.8.8'}));assert.equal(closes,3)
  console.log('PASS WOL: exact magic packet, explicit local NIC/directed broadcast, one send, no retry, disabled/unsupported codes, real errors retained')
  const{MasterSerialClient}=load('kbm/src/services/master-serial.ts'),{ProgressiveDecoder,encodePacket}=load('kbm/src/services/protocol.ts')
  let deliver,status=0,result=0,bridge=0;const writes=[],decoder=new ProgressiveDecoder()
  global.window={kbmNative:{logs:{record:()=>{}},master:{connect:async()=>true,onConnection:()=>{},onError:()=>{},onData:cb=>{deliver=cb},write:async bytes=>{
    for(const event of decoder.feed(bytes)){if(event.type!=='packet')continue;const p=event.packet;writes.push(p)
      if(p.op===0x71)deliver(encodePacket({op:0xf1,payload:Uint8Array.from({length:21},(_,i)=>i===1||i===2?3:0)}))
      if(p.op===0x77)deliver(encodePacket({op:0xf7,payload:Uint8Array.of(0,0,0,0,0)}))
      if(p.op===0x73)deliver(encodePacket({op:0xf3,payload:Uint8Array.of(p.payload[0],p.payload[1],p.payload[2],bridge,0xe1,status,result)}))
    }
  }}}}
  const client=new MasterSerialClient();assert(await client.initialize())
  for(result=0;result<=6;result++)assert.deepEqual(await client.wakePc(result%2),{code:result})
  status=1;assert.deepEqual(await client.wakePc(0),{code:5}) // Old slave firmware.
  status=3;await assert.rejects(client.wakePc(0),/status 3/)
  status=0;bridge=9;await assert.rejects(client.wakePc(1),/bridge status 9/)
  for(const write of writes.filter(p=>p.op===0x73)){assert.equal(write.payload[3],0x61);assert.equal(write.payload.length,4)}
  client.handleDisconnected()
  console.log('PASS USB wake: per-port route, result codes0..6, old firmware ignored, bridge/validation errors retained, no HID/reboot packets')
})().catch(error=>{console.error(error);process.exitCode=1})
