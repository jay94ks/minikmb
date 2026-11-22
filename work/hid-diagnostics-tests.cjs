// Mocked transports/clock only. No COM, HID, USB wake, restart or OS shutdown.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),{createRequire}=require('node:module')
const root=path.resolve(__dirname,'..'),ts=require('../kbm/node_modules/typescript'),cache=new Map()
function load(relative){const file=path.resolve(root,relative);if(cache.has(file))return cache.get(file);const exports={};cache.set(file,exports);const local=createRequire(file)
 const code=ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText
 vm.runInThisContext(`(function(exports,require){${code}\n})`,{filename:file})(exports,name=>name.startsWith('.')?load(path.relative(root,path.resolve(path.dirname(file),name+'.ts'))):local(name));return exports}
function diag(port,flags=27,fields={}){const p=Buffer.alloc(53);p[0]=port;p[1]=1;p[2]=flags;p[3]=fields.queue??0;p[4]=fields.report??0;p.writeUInt32LE(fields.uptime??100,9);p.writeUInt32LE(fields.resets??1,13);p.writeUInt32LE(fields.submitted??0,21);p.writeUInt32LE(fields.completed??0,25);p.writeUInt32LE(fields.wait??0,29);p.writeUInt32LE(fields.rejected??0,37);return{op:0xfa,payload:new Uint8Array(p)}}
function boot(port){const p=new Uint8Array(17);p[0]=port;p[1]=1;p[2]=3;p[3]=1;p[4]=8;p[5]=1;return{op:0xf9,payload:p}}
const flush=async()=>{for(let n=0;n<10;n++)await Promise.resolve()}
;(async()=>{
 const {decodeHidDiagnostics}=load('kbm/src/shared/hid-diagnostics.ts'),{HidDiagnostics}=load('kbm/src-electron/hid-diagnostics.ts'),{slaveBoot}=load('kbm/src/shared/boot.ts')
 assert.equal(slaveBoot(boot(1)).firmware,0x0801);assert.equal(slaveBoot({...boot(0),payload:new Uint8Array(17)}),undefined)
 assert.equal(decodeHidDiagnostics(diag(1)).snapshot.ready,true)
 for(const packet of[{op:0xfa,payload:new Uint8Array(2)},diag(2),diag(0,128),diag(0,0,{queue:33}),diag(0,0,{report:4})])assert.equal(decodeHidDiagnostics(packet),undefined)
 let now=10000,logs=[],alarms=[]
 const monitor=new HidDiagnostics((...args)=>logs.push(args),alarm=>alarms.push(alarm),()=>now)
 monitor.master(true);monitor.online(0,true);monitor.online(1,true)
 monitor.accept(diag(0));monitor.accept(diag(1));assert.equal(alarms.length,0)
 monitor.accept(diag(0,51,{queue:30,report:1,wait:2500,submitted:4,completed:3}))
 assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='hid-stalled'))
 assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='hid-queue-pressure'))
 const before=alarms.length;monitor.accept(diag(0,51,{queue:30,report:1,wait:2800}));assert.equal(alarms.length,before)
 assert.equal(monitor.state()[1].alarms.length,0)
 monitor.accept(diag(0));assert.equal(monitor.state()[0].alarms.length,0);assert(logs.some(row=>row[1]==='hid.recovered'))
 monitor.accept(diag(0,21));assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='usb-suspended'));assert(!monitor.state()[0].alarms.some(alarm=>alarm.code==='hid-stalled'))
 monitor.accept(diag(0,16));for(let n=0;n<6;n++){now+=1000;monitor.accept(diag(0,16))}
 assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='usb-unconfigured'))
 monitor.online(0,false);now+=1001;monitor.tick();assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='uart-offline'))
 monitor.boot(0,0x801);monitor.tick();assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='uart-offline')) // Reset isn't recovery.
 monitor.online(0,true);monitor.accept(diag(0));assert.equal(monitor.state()[0].bootCount,1);assert.equal(monitor.state()[0].alarms.length,0)
 now+=5000;monitor.tick();assert(monitor.state()[0].alarms.some(alarm=>alarm.code==='diagnostics-stale'))
 monitor.master(false);assert.equal(monitor.state()[0].alarms.length,0)
 const {RoutedTransport}=load('kbm/src-electron/routed-transport.ts'),{ProgressiveDecoder}=load('kbm/src/services/protocol.ts')
 const sent=[],transport=new RoutedTransport(async bytes=>sent.push(new ProgressiveDecoder().feed(bytes)[0].packet))
 const request=(port,id)=>({op:0x73,payload:Uint8Array.of(port,id,0,0)})
 const a=transport.request(request(0,1)),b=transport.request(request(0,2)),c=transport.request(request(1,3))
 const rejectedA=assert.rejects(a,/restarted/),rejectedB=assert.rejects(b,/restarted/)
 transport.resetPort(0);await rejectedA;await rejectedB
 const reply=packet=>({op:0xf3,payload:Uint8Array.of(packet.payload[0],packet.payload[1],packet.payload[2],0,0x80,0,0)})
 transport.accept(reply(sent[0]));transport.accept(reply(sent[1]));assert.equal((await c).payload[1],3)
 const next=transport.request(request(0,4));await flush();assert.equal(sent.length,3);transport.accept(reply(sent[2]));await next
 const {SpecialInput}=load('kbm/src-electron/special-input.ts');let unlock;const gate=new Promise(resolve=>unlock=resolve),keys=[]
 const input=new SpecialInput(async(port,op,payload)=>{keys.push([port,op,...payload])},()=>true,()=>{},()=>gate)
 const job=input.chord(0,{modifiers:8,key:'KeyR'}),jobRejected=assert.rejects(job,/취소/);await flush();input.restarted(0);unlock();await jobRejected;assert(!input.blocked(0));assert(keys.every(row=>row[1]===16))
 global.window={kbmNative:{master:{write:async()=>{}},logs:{record(){}}}}
 const {MasterSerialClient}=load('kbm/src/services/master-serial.ts'),client=new MasterSerialClient()
 client.state.connected=true;client.state.protocolVersion=3;client.state.online=[true,true]
 const old=client.route(0,{op:0,payload:new Uint8Array()}),queued=client.route(0,{op:0,payload:new Uint8Array()}),other=client.route(1,{op:0,payload:new Uint8Array()})
 const rejectOld=assert.rejects(old,/재부팅/),rejectQueued=assert.rejects(queued,/재부팅/)
 let bootPort;client.onSlaveBoot(port=>bootPort=port);client.handlePacket(boot(0));await rejectOld;await rejectQueued
 assert.equal(bootPort,0);assert.equal(client.queues[0].length,0);assert.equal(client.state.online[1],true)
 const active=client.active[1];client.handlePacket({op:0xf3,payload:Uint8Array.of(1,active.sequence&255,active.sequence>>>8,0,0x80,0,0)});await other
 console.log('PASS boot/session + HID diagnostics: strict codec, USB/HID/UART separation, alarms/debounce/recovery, reset is not recovery, per-port native/renderer queue cancellation, stale ACK isolation, no old sequence UP after reboot; no hardware input')
})().catch(error=>{console.error(error);process.exitCode=1})
