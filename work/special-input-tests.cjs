// All transports mocked: no physical HID, secure-attention sequence or power.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict'),{createRequire}=require('node:module')
const ts=require('../kbm/node_modules/typescript'),root=path.resolve(__dirname,'..'),cache=new Map()
function load(relative){const file=path.resolve(root,relative);if(cache.has(file))return cache.get(file);const exports={};cache.set(file,exports);const local=createRequire(file)
 const code=ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText
 vm.runInThisContext(`(function(exports,require){${code}\n})`,{filename:file})(exports,name=>name.startsWith('.')?load(path.relative(root,path.resolve(path.dirname(file),name+'.ts'))):local(name));return exports}
const deferred=()=>{let resolve;return {promise:new Promise(r=>{resolve=r}),resolve:()=>resolve()}}
;(async()=>{
 const{SpecialInput,SystemControlRejected}=load('kbm/src-electron/special-input.ts'),{chordUsages,specialPresets}=load('kbm/src/shared/special-input.ts'),{keyboardCodeToUsage}=load('kbm/src/services/hid-keyboard.ts')
 const{systemControls,systemControl}=load('kbm/src/shared/system-controls.ts'),{virtualKeyboardRows}=load('kbm/src/shared/virtual-keyboard.ts')
 const defined=[...fs.readFileSync(path.join(root,'common/system_controls.def'),'utf8').matchAll(/KBM_SYSTEM_USAGE\((0x[\da-f]+)\)/g)].map(match=>Number(match[1]))
 assert.equal(defined.length,40);assert.deepEqual(systemControls.map(control=>control.usage),defined)
 for(const invalid of[0,0x80,0x90,0x99,0xab,0xff,1.5,'129'])assert.throws(()=>systemControl(invalid))
 assert(systemControl(0x8e).confirm&&systemControl(0x8f).confirm)
 for(const row of virtualKeyboardRows)for(const key of row)assert.equal(chordUsages({modifiers:0,key:key.code}).length,1)
 assert(!virtualKeyboardRows.flat().some(key=>key.code==='Fn'))
 assert.deepEqual(chordUsages({modifiers:3,key:'Delete'}),[0xe0,0xe2,0x4c]);assert.deepEqual(chordUsages({modifiers:8,key:''}),[0xe3])
 assert.equal(keyboardCodeToUsage('MetaRight'),0xe7);assert.equal(keyboardCodeToUsage('F13'),0x68);assert.equal(keyboardCodeToUsage('F24'),0x73);assert.equal(keyboardCodeToUsage('F25'),undefined)
 for(const bad of[{},null,{modifiers:16,key:'Delete'},{modifiers:0,key:''},{modifiers:0,key:'Power'},{modifiers:0,key:'__proto__'}])assert.throws(()=>chordUsages(bad))
 for(const preset of specialPresets)assert(chordUsages(preset).length<=5)
 let events=[],logs=[]
 const service=new SpecialInput(async(port,op,payload)=>{events.push([port,op,...payload])},()=>true,(...args)=>logs.push(args),async()=>{})
 await service.chord(1,{modifiers:3,key:'Delete'})
 assert.deepEqual(events,[[1,16,224],[1,16,226],[1,16,76],[1,17,76],[1,17,226],[1,17,224]])
 assert(!JSON.stringify(logs).includes('224'));events=[]
 await assert.rejects(service.power(0,false));assert.equal(events.length,0)
 await service.power(0,true);assert.deepEqual(events,[[0,18,129],[0,19,129]])
 assert.deepEqual(logs.filter(row=>row[1].startsWith('input.power_')).map(row=>row[1]),['input.power_started','input.power_down_accepted','input.power_up_accepted','input.power_finished'])
 assert(logs.filter(row=>row[1].includes('_accepted')).every(row=>row[2].hostActionConfirmed===false))
 const powerLogs=[];let failedPowerUp=true
 const failedPower=new SpecialInput(async(port,op)=>{if(op===0x12)throw Error('lost power DOWN ACK');if(op===0x13&&failedPowerUp)throw Error('lost power UP ACK')},()=>true,(...args)=>powerLogs.push(args),async()=>{})
 await assert.rejects(failedPower.power(0,true),/lost power DOWN/);assert(failedPower.state()[0].needsRelease)
 assert.deepEqual(powerLogs.map(row=>row[1]),['input.power_started','input.power_down_failed','input.power_up_failed','input.power_finished'])
 assert(powerLogs.at(-1)[2].needsRelease);failedPowerUp=false;await failedPower.release(0)
 assert(powerLogs.some(row=>row[1]==='input.power_up_accepted'&&row[2].recovery===true))
 events=[]
 for(const control of systemControls){
  if(control.confirm){await assert.rejects(service.system(0,control.usage,false));assert.equal(events.length,0)}
  await service.system(1,control.usage,true)
  assert.deepEqual(events,[[1,18,control.usage],[1,19,control.usage]]);events=[]
 }
 const rejectedEvents=[]
 const unsupported=new SpecialInput(async(port,op,payload)=>{rejectedEvents.push([port,op,...payload]);throw new SystemControlRejected('unsupported','explicit negative ACK')},()=>true,()=>{},async()=>{})
 await assert.rejects(unsupported.system(0,0x8e,true),/negative ACK/)
 assert.deepEqual(rejectedEvents,[[0,18,0x8e]]);assert(!unsupported.blocked(0))
 const lostSystemEvents=[]
 const lostSystem=new SpecialInput(async(port,op,payload)=>{lostSystemEvents.push([port,op,...payload]);if(op===18)throw Error('lost system DOWN ACK')},()=>true,()=>{},async()=>{})
 await assert.rejects(lostSystem.system(1,0x8f,true),/lost system DOWN/)
 assert.deepEqual(lostSystemEvents,[[1,18,0x8f],[1,19,0x8f]]);assert(!lostSystem.blocked(1))
 for(const failedAt of[0,1,2]){
  events=[];let downs=0
  const failed=new SpecialInput(async(port,op,payload)=>{events.push([port,op,...payload]);if(op===16&&downs++===failedAt)throw Error('lost DOWN ACK')},()=>true,()=>{},async()=>{})
  await assert.rejects(failed.chord(0,{modifiers:3,key:'Delete'}),/lost DOWN/)
  const pressed=[224,226,76].slice(0,failedAt+1)
  assert.deepEqual(events,pressed.map(key=>[0,16,key]).concat([...pressed].reverse().map(key=>[0,17,key])))
  assert(!failed.state()[0].needsRelease)
 }
 events=[];let upFailed=true
 const uncertain=new SpecialInput(async(port,op,payload)=>{events.push([port,op,...payload]);if(op===17&&payload[0]===226&&upFailed)throw Error('lost UP ACK')},()=>true,()=>{},async()=>{})
 await assert.rejects(uncertain.chord(1,{modifiers:3,key:'Delete'}),/lost UP/)
 assert.deepEqual(events.slice(-3),[[1,17,76],[1,17,226],[1,17,224]])
 assert(uncertain.state()[1].needsRelease&&uncertain.blocked(1));assert.throws(()=>uncertain.chord(1,{modifiers:8,key:''}),/해제/)
 upFailed=false;await uncertain.release(1);assert(!uncertain.blocked(1));assert.deepEqual(events.at(-1),[1,127])
 const hold=deferred();let waits=0;events=[]
 const concurrent=new SpecialInput(async(port,op,payload)=>events.push([port,op,...payload]),()=>true,()=>{},()=>++waits===1?hold.promise:Promise.resolve())
 const first=concurrent.chord(0,{modifiers:8,key:''});await Promise.resolve();await Promise.resolve()
 assert(concurrent.blocked(0));assert.throws(()=>concurrent.chord(0,{modifiers:8,key:''}),/진행/)
 await concurrent.chord(1,{modifiers:8,key:'KeyL'});assert(events.some(event=>event[0]===1&&event[1]===17));hold.resolve();await first
 const gate=deferred();events=[]
 const closing=new SpecialInput(async(port,op,payload)=>{events.push([port,op,...payload]);if(op===16)await gate.promise},()=>true,()=>{},async()=>{})
 const active=closing.chord(1,{modifiers:3,key:'Delete'});const rejected=assert.rejects(active,/취소/)
 const stopping=closing.stop();gate.resolve();await stopping;await rejected
 assert.deepEqual(events,[[1,16,224],[1,17,224]])
 console.log('PASS special input: Win/F13..24, Ctrl+Alt+Del, atomic DOWN/reverse UP, lost ACK cleanup, all-UP attempts, uncertain-state lock/recovery, dual ports, stop cancellation, power confirmation, no physical HID')
})().catch(error=>{console.error(error);process.exitCode=1})
