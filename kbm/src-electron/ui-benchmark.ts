// Opt-in measurement of the real empty-channel UI, never a device test.
import { app, BrowserWindow, ipcMain, Menu, session, screen } from 'electron'
import path from 'node:path'
import { existsSync, mkdirSync, writeFileSync } from 'node:fs'

export function prepareUiBenchmark(args: string[]): string | undefined {
  if (!args.includes('--ui-benchmark')) return
  if (args.length !== 2 || args[0] !== '--ui-benchmark' || !path.isAbsolute(args[1]!)) {
    throw new Error('--ui-benchmark requires only a new absolute output directory')
  }
  const directory = args[1]!
  // Exclusive directory creation; no credentials/settings from a normal profile.
  mkdirSync(directory)
  const profile = path.join(directory, 'profile')
  mkdirSync(profile)
  app.setPath('userData', profile)
  app.setPath('sessionData', profile)
  return directory
}

export async function runUiBenchmark(directory: string, preload: string, load: (w: BrowserWindow) => Promise<void>): Promise<void> {
  const phase = (value: string) => writeFileSync(path.join(directory, 'phase.json'), JSON.stringify({ phase: value }))
  phase('app-ready')
  Menu.setApplicationMenu(null)
  const ses = session.defaultSession
  ses.setPermissionCheckHandler(() => false)
  ses.setPermissionRequestHandler((_w, _permission, reply) => reply(false))
  // Never register the production device/network/HID IPC handlers in this mode.
  let scanned = false
  const inventory = { boards: [], error: '', scannedAt: 0 }
  ipcMain.handle('minikmb:scan', () => { scanned = true; return inventory })
  ipcMain.handle('minikmb:state', () => inventory)
  ipcMain.handle('minikmb:keyboard-capture', () => ({ active: false, uid: '', error: '' }))
  ipcMain.handle('minikmb:release-input', () => {})
  ipcMain.handle('minikmb:video-stop', () => {})
  const window = new BrowserWindow({ width: 1280, height: 820, minWidth: 840, minHeight: 560,
    useContentSize: true, show: false, paintWhenInitiallyHidden: true, focusable: false, autoHideMenuBar: true, backgroundColor: '#080d15',
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true, preload, backgroundThrottling: false } })
  const painted = new Promise<void>(resolve => window.once('ready-to-show', () => resolve()))
  // Reapply after the HWND has its actual monitor DPI (constructor rounding can differ).
  window.setContentSize(1280, 820)
  window.setIgnoreMouseEvents(true)
  window.webContents.on('before-input-event', event => event.preventDefault())
  const timeout = setTimeout(() => app.quit(), 30000)
  const stop = setInterval(() => { if (existsSync(path.join(directory, 'stop'))) app.quit() }, 250)
  window.on('closed', () => { clearInterval(stop); clearTimeout(timeout) })
  phase('window-created')
  await load(window)
  phase('page-loaded')
  const scanDeadline = Date.now() + 3000
  while (!scanned && Date.now() < scanDeadline) await new Promise(resolve => setTimeout(resolve, 10))
  if (!scanned) throw new Error('Real channel page did not complete its passive inventory request')
  // An occluded inactive Chromium window may suspend requestAnimationFrame.
  // Wait for the actual Vue page/scan to settle, then explicitly read it back.
  await window.webContents.executeJavaScript(`new Promise((resolve, reject) => {
    const deadline = performance.now() + 3000;
    const check = () => {
      if (document.querySelector('.mini-content .mini-eyebrow') && !document.querySelector('.q-btn--loading')) resolve(true);
      else if (performance.now() > deadline) reject(new Error('Channel page did not settle'));
      else setTimeout(check, 16);
    }; check();
  })`)
  phase('page-settled')
  await painted
  window.showInactive()
  phase('first-paint')
  const image = await window.webContents.capturePage()
  if (image.isEmpty()) throw new Error('Empty benchmark frame')
  writeFileSync(path.join(directory, 'frame.png'), image.toPNG(), { flag: 'wx' })
  // Restore the normal policy before the settle/idle interval is measured.
  window.webContents.setBackgroundThrottling(true)
  const size = image.getSize(), scale = screen.getDisplayMatching(window.getBounds()).scaleFactor
  writeFileSync(path.join(directory, 'ready.json'), JSON.stringify({ app: 'electron',
    width_px: size.width, height_px: size.height, scale, content_dip: window.getContentSize(),
    devices: 'disabled', frame: 'frame.png', electron: process.versions.electron }), { flag: 'wx' })
  phase('ready')
}
