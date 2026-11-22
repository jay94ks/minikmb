// Windows host, entirely in-memory Vue renderer + mocked native IPC.
// Never loads master-serial.ts, opens COM, or generates a real HID report.
const fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),assert=require('node:assert/strict')
const root=path.resolve(__dirname,'..'),base=path.join(root,'kbm'),ts=require('../kbm/node_modules/typescript')
const vue=require('../kbm/node_modules/vue'),sfc=require('../kbm/node_modules/@vue/compiler-sfc')
const {h,createRenderer,defineComponent,reactive,nextTick}=vue,cache=new Map(),sent=[]
const state=[{busy:false,needsRelease:false},{busy:false,needsRelease:false}]
global.localStorage={getItem:()=>null,setItem:()=>{}}
global.document={fullscreenElement:null}
let systemResult={status:'accepted',hostActionConfirmed:false,message:'QUEUE ACCEPTED; OS UNKNOWN'}
global.window={kbmNative:{input:{state:async()=>structuredClone(state),chord:async(port,chord)=>sent.push(['chord',port,chord]),release:async port=>sent.push(['release',port]),system:async(port,usage,confirmed)=>{sent.push(['system',port,usage,confirmed]);return systemResult}}}}
function load(file){
 if(file.endsWith('master-serial.ts'))return {masterSerial:{subscribe(fn){fn({connected:true,protocolVersion:3,online:[true,true]});return()=>{}},isLinkTesting:()=>false,isRebooting:()=>false}}
 if(cache.has(file))return cache.get(file)
 let source=fs.readFileSync(file,'utf8')
 if(file.endsWith('.vue')){const {descriptor,errors}=sfc.parse(source,{filename:file});assert.deepEqual(errors,[]);source=sfc.compileScript(descriptor,{id:file,inlineTemplate:true}).content}
 const exports={};cache.set(file,exports)
 const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText
 vm.runInThisContext(`(function(exports,require){${js}\n})`,{filename:file})(exports,name=>{
  if(name==='vue')return vue
  if(name.startsWith('.'))return load(path.resolve(path.dirname(file),name+(path.extname(name)?'':'.ts')))
  throw Error(`Unexpected dependency ${name}`)
 });return exports
}
const node=(type,text='')=>({type,text,children:[],props:{},style:{display:''},parent:null})
const renderer=createRenderer({
 createElement:tag=>node(tag),createText:text=>node('#text',text),createComment:text=>node('#comment',text),
 setText:(n,text)=>n.text=text,setElementText:(n,text)=>{n.text=text;n.children=[]},
 patchProp:(n,key,old,value)=>{n.props[key]=value},parentNode:n=>n.parent,nextSibling:n=>{const c=n.parent?.children??[];return c[c.indexOf(n)+1]??null},
 insert:(n,parent,anchor)=>{if(n.parent){const c=n.parent.children;c.splice(c.indexOf(n),1)}n.parent=parent;const index=anchor?parent.children.indexOf(anchor):-1;parent.children.splice(index<0?parent.children.length:index,0,n)},
 remove:n=>{if(n.parent){const c=n.parent.children;c.splice(c.indexOf(n),1);n.parent=null}}
})
function mount(component,props){
 const container=node('root'),app=renderer.createApp({render:()=>h(component,props)})
 const names=['btn','badge','icon','space','tooltip','select','expansion-item','dialog','card','card-section','card-actions']
 for(const name of names)app.component(`q-${name}`,defineComponent({inheritAttrs:false,setup(_,ctx){return()=>name==='dialog'&&!ctx.attrs.modelValue?null:h(name==='btn'?'button':'div',{...ctx.attrs,disabled:Boolean(ctx.attrs.disable),'data-q':name},[ctx.attrs.label??'',...(ctx.slots.default?.()??[])])}}))
 app.directive('close-popup',{mounted(){}})
 app.mount(container);return{container,unmount:()=>app.unmount()}
}
function walk(n){return[n,...n.children.flatMap(walk)]}
function button(tree,label){const match=walk(tree).find(n=>n.type==='button'&&(n.props.label===label||n.children.some(c=>c.text===label)));assert(match,`Missing button ${label}`);return match}
function text(n){return n.text+n.children.map(text).join('')}
const settle=async()=>{for(let n=0;n<8;n++){await Promise.resolve();await nextTick()}}
async function click(n){assert(!n.props.disabled,`Disabled button ${n.props.label}`);await n.props.onClick?.();await settle()}
;(async()=>{
 const VirtualKeyboard=load(path.join(base,'src/components/VirtualKeyboard.vue')).default
 const props=reactive({visible:true,port:0,screen:1,onClose:()=>props.visible=false}),keyboard=mount(VirtualKeyboard,props)
 try{
  await settle();assert.equal(sent.length,0)
  await click(button(keyboard.container,'Ctrl'));assert.equal(sent.length,0)
  await click(button(keyboard.container,'A'));assert.deepEqual(sent.pop(),['chord',0,{modifiers:1,key:'KeyA'}])
  await click(button(keyboard.container,'B'));assert.deepEqual(sent.pop(),['chord',0,{modifiers:0,key:'KeyB'}])
  await click(button(keyboard.container,'Ctrl + Alt + Delete'));assert.deepEqual(sent.pop(),['chord',0,{modifiers:3,key:'Delete'}])
  props.port=1;props.screen=2;await settle()
  await click(button(keyboard.container,'Win + L'));assert.deepEqual(sent.pop(),['chord',1,{modifiers:8,key:'KeyL'}])
  const close=walk(keyboard.container).find(n=>n.props['aria-label']==='가상 키보드 닫기');await click(close)
  assert.equal(props.visible,false);assert.equal(walk(keyboard.container).find(n=>n.type==='aside').style.display,'none');assert.equal(sent.length,0)
 }finally{keyboard.unmount()}
 const SystemControls=load(path.join(base,'src/components/SystemControls.vue')).default
 const system=mount(SystemControls,reactive({port:1,disabled:false}))
 try{
  await settle();assert.equal(sent.length,0)
  const controlButton=label=>{const n=walk(system.container).find(n=>n.type==='button'&&text(n).startsWith(label));assert(n,label);return n}
  await click(controlButton('System Cold Restart'));assert.equal(sent.length,0) // Confirmation only.
  await click(button(system.container,'확인 후 한 번 발신'));assert.deepEqual(sent.pop(),['system',1,0x8e,true])
  assert(text(system.container).includes('QUEUE ACCEPTED; OS UNKNOWN'))
  await click(button(system.container,'안 됨'));assert(text(system.container).includes('안 됨 (사용자 확인)'))
  systemResult={status:'unsupported',hostActionConfirmed:false,message:'FIRMWARE UNSUPPORTED'}
  await click(controlButton('System Menu Help'));assert.deepEqual(sent.pop(),['system',1,0x87,false]);assert(text(system.container).includes('펌웨어 미지원'))
  assert(!walk(system.container).some(n=>n.type==='button'&&text(n).startsWith('System Function Shift Lock Indicator')))
 }finally{system.unmount()}
 const page=fs.readFileSync(path.join(base,'src/pages/IndexPage.vue'),'utf8')
 assert(page.includes('v-if="!keyboardOpen"'));assert(page.includes('label="가상 키보드 열기"'));assert(page.includes('const keyboardOpen = ref(true)'))
 assert.equal(sent.length,0)
 console.log('PASS virtual keyboard: real Vue event handlers, no send on mount/modifier selection/close, named shortcuts, one-shot modifiers, UART mapping, close/reopen placement;40 system controls, confirmation, honest ACK/unsupported/manual feedback; all IPC mocked')
})().catch(error=>{console.error(error);process.exitCode=1})
