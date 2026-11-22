import type { PnpVideoDevice } from '../types/native-api'
import {
  loadCaptureAssignments, loadCaptureMode, resolveCaptureAssignments, validCaptureMode,
  type CaptureAssignments, type CaptureMode
} from './capture-preferences'

const BLACKLIST_STORAGE_KEY = 'kbm.capture-blacklist.v1'
const DEFAULT_BLACKLIST: CaptureBlacklist = {
  names: ['surface camera', 'surface ir camera', 'integrated camera', 'webcam'],
  usbIds: ['045e:0c1e']
}

export interface CaptureBlacklist {
  names: string[]
  usbIds: string[]
}

export interface VideoDeviceInventory {
  deviceId: string
  groupId: string
  label: string
  pnp?: PnpVideoDevice
  usbId: string
  blacklisted: boolean
}

export interface CaptureSlot {
  deviceId: string
  groupId: string
  label: string
  usbId: string
  instanceId: string
  stream: MediaStream
  width: number
  height: number
  frameRate: number
  requestedMode: CaptureMode
}

function normalizeName(value: string): string {
  return value.toLowerCase().replace(/\([^)]*[0-9a-f]{4}:[0-9a-f]{4}[^)]*\)/gi, '')
    .replace(/[^a-z0-9가-힣]+/g, ' ').trim()
}

function normalizeUsbId(value: string): string {
  const match = /(?:vid_)?([0-9a-f]{4})\s*[:&/_-]?\s*(?:pid_)?([0-9a-f]{4})/i.exec(value)
  return match ? `${match[1]?.toLowerCase()}:${match[2]?.toLowerCase()}` : ''
}

function unique(values: string[]): string[] {
  return [...new Set(values.map(value => value.trim().toLowerCase()).filter(Boolean))]
}

export function loadCaptureBlacklist(): CaptureBlacklist {
  try {
    const parsed = JSON.parse(localStorage.getItem(BLACKLIST_STORAGE_KEY) ?? '') as Partial<CaptureBlacklist>
    return {
      names: unique(Array.isArray(parsed.names) ? parsed.names : DEFAULT_BLACKLIST.names),
      usbIds: unique(Array.isArray(parsed.usbIds) ? parsed.usbIds.map(normalizeUsbId) : DEFAULT_BLACKLIST.usbIds)
    }
  } catch {
    return { names: [...DEFAULT_BLACKLIST.names], usbIds: [...DEFAULT_BLACKLIST.usbIds] }
  }
}

export function saveCaptureBlacklist(blacklist: CaptureBlacklist): CaptureBlacklist {
  const normalized = {
    names: unique(blacklist.names),
    usbIds: unique(blacklist.usbIds.map(normalizeUsbId))
  }
  localStorage.setItem(BLACKLIST_STORAGE_KEY, JSON.stringify(normalized))
  return normalized
}

export function defaultCaptureBlacklist(): CaptureBlacklist {
  return { names: [...DEFAULT_BLACKLIST.names], usbIds: [...DEFAULT_BLACKLIST.usbIds] }
}

function isBlacklisted(label: string, pnp: PnpVideoDevice | undefined,
  blacklist: CaptureBlacklist): boolean {
  const names = normalizeName(`${label} ${pnp?.name ?? ''}`)
  const usbId = pnp ? `${pnp.vendorId}:${pnp.productId}` : normalizeUsbId(label)
  return blacklist.names.some(entry => names.includes(normalizeName(entry))) ||
    (usbId !== ':' && blacklist.usbIds.includes(usbId))
}

export async function listVideoDeviceInventory(
  blacklist = loadCaptureBlacklist()
): Promise<VideoDeviceInventory[]> {
  const [mediaDevices, pnpDevices] = await Promise.all([
    navigator.mediaDevices.enumerateDevices(),
    window.kbmNative.video.listPnp()
  ])
  const availablePnp = [...pnpDevices].sort((a, b) => a.instanceId.localeCompare(b.instanceId))

  return mediaDevices.filter(device => device.kind === 'videoinput').map(device => {
    const mediaName = normalizeName(device.label)
    const matches = availablePnp.filter(pnp => {
      const pnpName = normalizeName(pnp.name)
      return mediaName === pnpName || mediaName.includes(pnpName) || pnpName.includes(mediaName)
    })
    // Identical capture names cannot be correlated to Windows PnP paths by sort
    // order. Do not invent a physical identity. Browser deviceId is the local key.
    const pnp = matches.length === 1 ? matches[0] : undefined
    if (pnp) availablePnp.splice(availablePnp.indexOf(pnp), 1)
    const usbId = pnp?.vendorId && pnp.productId
      ? `${pnp.vendorId}:${pnp.productId}`
      : normalizeUsbId(device.label)
    return {
      deviceId: device.deviceId,
      groupId: device.groupId,
      label: device.label || 'Video input',
      ...(pnp ? { pnp } : {}),
      usbId,
      blacklisted: isBlacklisted(device.label, pnp, blacklist)
    }
  })
}

async function waitForFirstFrame(stream: MediaStream): Promise<void> {
  const track = stream.getVideoTracks()[0]
  if (!track) throw new Error('No video track')
  const video = document.createElement('video')
  video.muted = true
  video.playsInline = true
  video.srcObject = stream
  let callbackId = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let ended: () => void = () => {}
  try {
    await new Promise<void>((resolve, reject) => {
      ended = () => reject(new Error('Video source ended before its first frame'))
      track.addEventListener('ended', ended, { once: true })
      timer = setTimeout(() => reject(new Error('Video source did not deliver a frame within 5 seconds')), 5_000)
      const frame = () => {
        if (track.readyState === 'ended') {
          ended()
        } else if (video.videoWidth > 2 && video.videoHeight > 2) {
          resolve()
        } else {
          callbackId = video.requestVideoFrameCallback(frame)
        }
      }
      callbackId = video.requestVideoFrameCallback(frame)
      void video.play().catch(reject)
    })
  } finally {
    if (timer) clearTimeout(timer)
    track.removeEventListener('ended', ended)
    video.cancelVideoFrameCallback(callbackId)
    video.pause()
    video.srcObject = null
  }
}

export async function openCaptureDevice(
  device: VideoDeviceInventory, requestedMode: CaptureMode = loadCaptureMode(device.deviceId)
): Promise<CaptureSlot> {
  if (!validCaptureMode(requestedMode)) throw new Error('너비·높이 2..8192, 목표 Hz 1..240을 입력하세요.')
  const resolutions = [
    { label: '1920×1080', width: 1920, height: 1080 },
    { label: '1280×720', width: 1280, height: 720 },
    { label: '640×480', width: 640, height: 480 },
    { label: 'default' }
  ]
  const automaticModes = resolutions.flatMap(resolution => [30, 24, undefined].map(frameRate => ({
    ...resolution,
    frameRate,
    label: `${resolution.label} ${frameRate === undefined ? 'default FPS' : `${frameRate} FPS`}`
  })))
  const modes = requestedMode.kind === 'auto' ? automaticModes : [{
    width: requestedMode.width, height: requestedMode.height, frameRate: requestedMode.frameRate,
    label: `${requestedMode.width}×${requestedMode.height} ${requestedMode.frameRate} FPS`
  }]
  const failures: string[] = []
  for (const mode of modes) {
    let stream: MediaStream | undefined
    try {
      // Chromium exposes resizeMode even though TypeScript's DOM types omit it.
      const video: MediaTrackConstraints & { resizeMode?: { exact: 'none' } } = {
        deviceId: { exact: device.deviceId }
      }
      if (mode.width !== undefined && mode.height !== undefined) {
        video.width = { exact: mode.width }
        video.height = { exact: mode.height }
        video.resizeMode = { exact: 'none' }
      }
      if (mode.frameRate !== undefined) video.frameRate = { exact: mode.frameRate }
      stream = await navigator.mediaDevices.getUserMedia({ audio: false, video })
      await waitForFirstFrame(stream)
      const settings = stream.getVideoTracks()[0]!.getSettings()
      if (requestedMode.kind === 'fixed' &&
          (settings.width !== requestedMode.width || settings.height !== requestedMode.height ||
          settings.frameRate === undefined || Math.abs(settings.frameRate - requestedMode.frameRate) > 0.05)) {
        throw new Error(`요청 모드와 실제 수신 모드가 다릅니다: ${settings.width}×${settings.height} ${settings.frameRate} FPS`)
      }
      console.info('Capture mode ready', device.label, mode.label, settings)
      return {
        deviceId: device.deviceId,
        groupId: device.groupId,
        label: device.label,
        usbId: device.usbId,
        instanceId: device.pnp?.instanceId ?? '',
        stream,
        width: settings.width ?? 0,
        height: settings.height ?? 0,
        frameRate: settings.frameRate ?? 0,
        requestedMode: { ...requestedMode }
      }
    } catch (error) {
      stream?.getTracks().forEach(track => track.stop())
      const reason = `${mode.label}: ${String(error)}`
      failures.push(reason)
      console.warn('Capture mode failed', device.label, reason)
      if (error instanceof DOMException &&
          ['NotAllowedError', 'SecurityError', 'NotFoundError'].includes(error.name)) throw error
      await new Promise(resolve => setTimeout(resolve, 150))
    }
  }
  throw new Error(failures.join(' / '))
}

export async function openCaptureDevices(
  blacklist = loadCaptureBlacklist(), savedAssignments = loadCaptureAssignments()
): Promise<{
  slots: Array<CaptureSlot | undefined>
  inventory: VideoDeviceInventory[]
  errors: string[]
  assignments: CaptureAssignments
}> {
  const inventory = await listVideoDeviceInventory(blacklist)
  const candidates = inventory.filter(device => !device.blacklisted)
  const assignments = resolveCaptureAssignments(savedAssignments, candidates.map(device => device.deviceId))
  const slots: Array<CaptureSlot | undefined> = [undefined, undefined]
  const errors: string[] = []
  for (const [index, id] of assignments.entries()) {
    const device = candidates.find(device => device.deviceId === id)
    if (!device) continue
    try {
      slots[index] = await openCaptureDevice(device)
    } catch (error) {
      errors.push(`${device.label}: ${String(error)}`)
    }
  }
  return { slots, inventory, errors, assignments }
}

export async function replaceCaptureMode(
  device: VideoDeviceInventory, previous: CaptureSlot | undefined, mode: CaptureMode
): Promise<{ slot: CaptureSlot | undefined; applied: boolean; error: string }> {
  if (!validCaptureMode(mode)) throw new Error('올바르지 않은 영상 모드입니다.')
  stopCaptureDevices([previous])
  // Give Windows drivers time to release the old capture handle.
  if (previous) await new Promise(resolve => setTimeout(resolve, 150))
  try {
    return { slot: await openCaptureDevice(device, mode), applied: true, error: '' }
  } catch (error) {
    let slot: CaptureSlot | undefined
    let message = String(error)
    if (previous) {
      try { slot = await openCaptureDevice(device, previous.requestedMode) }
      catch (restoreError) { message += ` / 이전 모드 복원 실패: ${String(restoreError)}` }
    }
    return { slot, applied: false, error: message }
  }
}

export function stopCaptureDevices(slots: Array<CaptureSlot | undefined>): void {
  for (const slot of slots) slot?.stream.getTracks().forEach(track => track.stop())
}
