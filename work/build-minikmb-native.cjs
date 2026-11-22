const fs = require('node:fs'), path = require('node:path'), { execFileSync } = require('node:child_process')
const root = path.resolve(__dirname, '..'), build = path.join(root, 'work/native-build')
const targets = [
  { name: 'minikmb-capture', libs: ['mf.lib', 'mfplat.lib', 'mfreadwrite.lib', 'mfuuid.lib', 'ole32.lib', 'cfgmgr32.lib'] },
  { name: 'minikmb-keyboard', libs: ['user32.lib'] }
].map(t => ({ ...t, source: path.join(root, `kbm/src-electron/native/${t.name}.cpp`), output: path.join(root, `kbm/src-electron/native-bin/${t.name}.exe`) }))
if (process.platform !== 'win32') throw Error('MiniKMB native capture build requires Windows SDK/MSVC')
const pending = targets.filter(t => !fs.existsSync(t.output) || fs.statSync(t.output).mtimeMs < fs.statSync(t.source).mtimeMs)
if (!pending.length) { console.log('Native capture/keyboard helpers are current'); process.exit(0) }
const vswhere = path.join(process.env['ProgramFiles(x86)'], 'Microsoft Visual Studio/Installer/vswhere.exe')
const vs = execFileSync(vswhere, ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'], { encoding: 'utf8' }).trim()
if (!vs) throw Error('MSVC C++ build tools not installed')
const setup = path.join(vs, 'VC/Auxiliary/Build/vcvars64.bat')
fs.mkdirSync(build, { recursive: true })
const environmentScript = path.join(build, 'msvc-environment.cmd')
fs.writeFileSync(environmentScript, `@call "${setup}" >nul\r\n@set\r\n`)
const text = execFileSync('cmd.exe', ['/d', '/c', environmentScript], { encoding: 'utf8', windowsHide: true })
const env = { ...process.env }
for (const line of text.split(/\r?\n/)) { const at = line.indexOf('='); if (at > 0) env[line.slice(0, at)] = line.slice(at + 1) }
fs.mkdirSync(path.dirname(targets[0].output), { recursive: true }); fs.mkdirSync(build, { recursive: true })
const compiler = path.join(env.VCToolsInstallDir, 'bin/Hostx64/x64/cl.exe')
for (const target of pending) {
  execFileSync(compiler, ['/nologo', '/std:c++20', '/O2', '/EHsc', '/W4', '/WX', '/utf-8', target.source, `/Fe:${target.output}`, ...target.libs], { cwd: build, env, stdio: 'inherit', windowsHide: true })
  console.log(`Built ${target.name}`)
}
