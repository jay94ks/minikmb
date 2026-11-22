// Only freshly-created regular files in the app's temporary folder are formatted.
const fs = require('node:fs'), vm = require('node:vm'), assert = require('node:assert/strict')
const ts = require('../kbm/node_modules/typescript'), exportsForTest = {}
const code = ts.transpileModule(fs.readFileSync(require('node:path').join(__dirname, '../kbm/src-electron/image-format.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
vm.runInThisContext(`(function(exports,require){${code}\n})`)(exportsForTest, require)
;(async () => {
  await assert.rejects(exportsForTest.makeFormattedImage(1, 1))
  await assert.rejects(exportsForTest.makeFormattedImage(1, 9 + 512 / 1048576, true))
  for (const format of [1, 2, 3]) {
    const image = await exportsForTest.makeFormattedImage(format, 64)
    try {
      const file = await fs.promises.open(image.filename, 'r')
      try {
        assert.equal((await file.stat()).size, 64 * 1024 * 1024)
        const bytes = Buffer.alloc(2048); assert.equal((await file.read(bytes)).bytesRead, bytes.length)
        if (format === 1) assert.equal(bytes.subarray(82, 90).toString(), 'FAT32   ')
        if (format === 2) assert.equal(bytes.subarray(3, 11).toString(), 'EXFAT   ')
        if (format === 3) assert.equal(bytes.readUInt16LE(1080), 0xef53)
      } finally { await file.close() }
    } finally { await image.close() }
    assert(!fs.existsSync(image.filename))
    console.log(`PASS ${['', 'FAT32', 'exFAT', 'EXT4'][format]} 64MiB temporary image creation, filesystem check/signature and cleanup; no SD/device writes`)
  }
  const exact = await exportsForTest.makeFormattedImage(2, 9 + 512 / 1048576, true)
  try { assert.equal((await fs.promises.stat(exact.filename)).size, 9 * 1048576 + 512) } finally { await exact.close() }
  console.log('PASS existing exFAT image format preserves non-MiB, 512-byte capacity; undersized FAT32 rejected before deletion')
})().catch(e => { console.error(e); process.exitCode = 1 })
