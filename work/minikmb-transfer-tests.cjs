const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict')
const ts = require('../kbm/node_modules/typescript'), api = {}
const js = ts.transpileModule(fs.readFileSync(path.resolve(__dirname, '../kbm/src-electron/transfer-progress.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText
vm.runInThisContext(`(function(exports){${js}\n})`)(api)
let now = 0
const meter = new api.TransferProgress(1048576, () => now)
assert.equal(meter.state().bytesPerSecond, null)
now = 1000; meter.advance(262144); assert.equal(meter.state().bytesPerSecond, 262144); assert.equal(meter.state().remainingSeconds, 3)
now = 2000; meter.advance(524288); assert.equal(meter.state().remainingSeconds, 2)
now = 7000; assert.equal(meter.state().bytesPerSecond, 0); assert.equal(meter.state().remainingSeconds, null)
now = 8000; meter.advance(786432); assert(meter.state().bytesPerSecond > 0)
now = 8250; meter.advance(1048576); assert.equal(meter.state().remainingSeconds, 0)
assert.throws(() => meter.advance(1048577)); assert.throws(() => meter.advance(5))
const next = new api.TransferProgress(10, () => now); assert.equal(next.state().bytesPerSecond, null)
console.log('PASS confirmed-byte throughput/ETA, warmup, rolling interval, stalls/resume, completion and fresh upload reset')
