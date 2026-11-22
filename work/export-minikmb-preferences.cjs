// Paste this file in the Electron MiniKMB DevTools console to download an export.
// Node tests require collect(); requiring the module never opens a device or writes a file.
(() => {
  function collect(storage) {
    const entries = {};
    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i);
      if (!/^minikmb\.(?:settings|video)\.(?:0x)?[0-9a-fA-F]{24}$/.test(key ?? '')) continue;
      if (Object.keys(entries).length >= 128) throw new Error('내보내기는 최대 64개 채널의 두 설정 키를 지원합니다.');
      const value = storage.getItem(key);
      if (typeof value !== 'string' || value.length > 65536) throw new Error('설정 값의 크기를 확인하세요.');
      entries[key] = value;
    }
    const result = { format: 'minikmb-electron-settings/v1', entries };
    if (new TextEncoder().encode(JSON.stringify(result)).length > 1024 * 1024) throw new Error('설정 내보내기가 1 MiB를 초과합니다.');
    return result;
  }
  if (typeof module !== 'undefined' && module.exports) { module.exports = { collect }; return; }
  const result = collect(localStorage);
  const url = URL.createObjectURL(new Blob([JSON.stringify(result, null, 2)], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url; link.download = 'minikmb-electron-settings.json'; link.hidden = true;
  document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
})();
