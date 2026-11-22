/** Electron main may expose instantiate(), but not the streaming Web API. */
export function ensureWasmStreaming(): void {
  if (typeof WebAssembly.instantiateStreaming === 'function') return
  if (typeof WebAssembly.instantiate !== 'function') {
    throw new Error('이 Electron 런타임에서 WebAssembly를 사용할 수 없습니다.')
  }
  // tcpip loads its small bundled WASM through a Response. Read that response
  // into memory and use the ordinary compiler; no renderer/security changes.
  WebAssembly.instantiateStreaming = async (source, imports) => {
    const response = await source
    return WebAssembly.instantiate(await response.arrayBuffer(), imports)
  }
}
