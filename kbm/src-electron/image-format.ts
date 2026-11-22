import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { Worker } from 'node:worker_threads'

export async function makeFormattedImage(format: 1 | 2 | 3, sizeMiB: number, existing = false): Promise<{ filename: string; close(): Promise<void> }> {
  const size = sizeMiB * 1024 * 1024
  if (![1, 2, 3].includes(format) || !Number.isSafeInteger(size) || size % 512 || sizeMiB > 32768 || (!existing && (!Number.isInteger(sizeMiB) || sizeMiB < 64))) throw new Error('새 이미지 용량은 64..32768 MiB입니다.')
  const minimum = format === 1 ? 64 : format === 2 ? 1 : 8
  if (sizeMiB < minimum) throw new Error(`이 파일시스템은 최소 ${minimum} MiB 이미지가 필요합니다. 더 큰 이미지를 새로 만드세요.`)
  const folder = await fs.mkdtemp(path.join(os.tmpdir(), 'minikmb-format-')), filename = path.join(folder, 'disk.img')
  const close = async () => { await fs.unlink(filename).catch(() => {}); await fs.rmdir(folder).catch(() => {}) }
  try {
    const file = await fs.open(filename, 'wx')
    try { await file.truncate(size) } finally { await file.close() }
    const drive = /^([a-z]):[\\/](.+)$/i.exec(filename)
    if (!drive) throw new Error('WSL에서 접근할 수 있는 로컬 드라이브 임시 경로가 필요합니다.')
    const linux = `/mnt/${drive[1]!.toLowerCase()}/${drive[2]!.replace(/\\/g, '/')}`
    const mkfs = format === 1 ? ['/usr/sbin/mkfs.fat', '-F', '32', '-s', '1'] : format === 2 ? ['/usr/sbin/mkfs.exfat'] : ['/usr/sbin/mke2fs', '-q', '-F', '-t', 'ext4', '-m', '0', '-E', 'lazy_itable_init=0,lazy_journal_init=0,nodiscard']
    const check = format === 1 ? ['/usr/sbin/fsck.fat', '-n'] : format === 2 ? ['/usr/sbin/fsck.exfat', '-n'] : ['/usr/sbin/e2fsck', '-fn']
    await new Promise<void>((resolve, reject) => {
      const worker = new Worker(`
        const { parentPort, workerData } = require('node:worker_threads');
        const { execFile } = require('node:child_process');
        const run = (args, next) => execFile('wsl.exe', ['--exec', ...args, workerData.filename], { windowsHide: true, timeout: 120000, maxBuffer: 1048576 },
          (error, stdout, stderr) => error ? parentPort.postMessage({ error: String(stderr || stdout || error.message).slice(-2048) }) : next());
        run(workerData.mkfs, () => run(workerData.check, () => parentPort.postMessage({ ok: true })));
      `, { eval: true, workerData: { filename: linux, mkfs, check } })
      let done = false
      const timer = setTimeout(() => { if (!done) { done = true; void worker.terminate(); reject(new Error('이미지 포맷 시간 초과')) } }, 250000)
      worker.once('message', (r: { ok?: boolean; error?: string }) => { if (done) return; done = true; clearTimeout(timer); r.ok ? resolve() : reject(new Error(`WSL 파일시스템 도구 실행 실패: ${r.error ?? ''}`)) })
      worker.once('error', e => { if (!done) { done = true; clearTimeout(timer); reject(e) } })
      worker.once('exit', code => { if (!done) { done = true; clearTimeout(timer); reject(new Error(`이미지 포맷 작업 종료 (${code})`)) } })
      worker.unref()
    })
    return { filename, close }
  } catch (e) { await close(); throw e }
}
