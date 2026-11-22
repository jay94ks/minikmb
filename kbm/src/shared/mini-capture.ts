export type MiniVideoMode = 'auto' | '1080' | '720' | '480' | { width: number; height: number; frameRate: number }
export interface MiniVideoInfo { uid: string; token: number; requestId: number; width: number; height: number; frameRate: number; frames: number; dropped: number }
export interface MiniVideoFrame extends MiniVideoInfo { sequence: number; jpeg: Uint8Array }
