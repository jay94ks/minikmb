const assert = require('node:assert/strict');
const { collect } = require('./export-minikmb-preferences.cjs');
const entries = new Map([
  ['minikmb.settings.010101010101010101010101', JSON.stringify({ config: { serverIp: '192.168.127.1', poolStart: 100, poolEnd: 199, leaseSeconds: 3600 }, rules: [], wol: { enabled: false, mac: '', localAddress: '' } })],
  ['minikmb.video.010101010101010101010101', '"1080"'],
  ['api-keys.json', 'do-not-read-or-export'],
  ['kbm.capture-preferences.v1', 'legacy RP2040 manual assignments'],
]);
const before = [...entries];
const source = { length: entries.size, key: n => [...entries.keys()][n], getItem: key => { assert.match(key, /^minikmb\./); return entries.get(key); } };
const result = collect(source);
assert.equal(result.format, 'minikmb-electron-settings/v1');
assert.equal(Object.keys(result.entries).length, 2);
assert.equal(result.entries['minikmb.video.010101010101010101010101'], '"1080"');
assert.equal(JSON.stringify(result).includes('do-not-read-or-export'), false);
assert.deepEqual([...entries], before);
assert.throws(() => collect({ length: 1, key: () => 'minikmb.video.010101010101010101010101', getItem: () => 'x'.repeat(65537) }));
process.stdout.write('MiniKMB preferences export: readonly prefix selection and bounds passed\n');
