#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <wrl/client.h>
#include <fcntl.h>
#include <io.h>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cwctype>
#include <mutex>
#include <string>
#include <vector>
using Microsoft::WRL::ComPtr;
constexpr DWORD video = static_cast<DWORD>(MF_SOURCE_READER_FIRST_VIDEO_STREAM);
constexpr DWORD maxFrame = 8 * 1024 * 1024;
std::wstring Lower(std::wstring s) { std::transform(s.begin(), s.end(), s.begin(), [](wchar_t c) { return static_cast<wchar_t>(std::towlower(c)); }); return s; }
std::wstring Parent(std::wstring link) {
  if (link.rfind(L"\\\\?\\", 0) != 0) return L"";
  link = link.substr(4); size_t a = link.find(L'#'), b = link.find(L'#', a + 1), c = link.find(L'#', b + 1);
  if (a == link.npos || b == link.npos || c == link.npos) return L"";
  link.resize(c); std::replace(link.begin(), link.end(), L'#', L'\\');
  DEVINST node{}, parent{}; wchar_t id[1024]{};
  if (CM_Locate_DevNodeW(&node, link.data(), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS ||
      CM_Get_Parent(&parent, node, 0) != CR_SUCCESS || CM_Get_Device_IDW(parent, id, 1024, 0) != CR_SUCCESS) return L"";
  return Lower(id);
}
struct Device { std::wstring link, root; ComPtr<IMFActivate> activation; };
std::vector<Device> Devices() {
  ComPtr<IMFAttributes> options; IMFActivate** list{}; UINT32 count{}; std::vector<Device> result;
  if (FAILED(MFCreateAttributes(&options, 1)) || FAILED(options->SetGUID(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)) ||
      FAILED(MFEnumDeviceSources(options.Get(), &list, &count))) return result;
  for (UINT32 i = 0; i < count; i++) {
    LPWSTR raw{};
    if (SUCCEEDED(list[i]->GetAllocatedString(MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, &raw, nullptr))) {
      auto link = Lower(raw);
      if ((link.find(L"usb#vid_345f&pid_2130&mi_00#") != link.npos || link.find(L"usb#vid_534d&pid_2130&mi_00#") != link.npos)) {
        auto root = Parent(link); if (!root.empty()) result.push_back({link, root, list[i]});
      }
      CoTaskMemFree(raw);
    }
    list[i]->Release();
  }
  CoTaskMemFree(list); return result;
}
std::string Json(const std::wstring& value) {
  std::string out = "\"";
  for (auto c : value) { if (c == L'\\' || c == L'"') out.push_back('\\'); if (c >= 32 && c < 127) out.push_back(static_cast<char>(c)); }
  return out + "\"";
}
struct Sample { HRESULT hr = S_OK; DWORD flags = 0; LONGLONG timestamp = 0; std::vector<BYTE> bytes; };
class Callback final : public IMFSourceReaderCallback {
  std::atomic<ULONG> refs{1}; std::mutex lock; Sample sample;
public:
  HANDLE ready = CreateEventW(nullptr, FALSE, FALSE, nullptr);
  ~Callback() { if (ready) CloseHandle(ready); }
  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** out) override {
    if (!out) return E_POINTER; *out = nullptr;
    if (iid != __uuidof(IUnknown) && iid != __uuidof(IMFSourceReaderCallback)) return E_NOINTERFACE;
    *out = static_cast<IMFSourceReaderCallback*>(this); AddRef(); return S_OK;
  }
  ULONG STDMETHODCALLTYPE AddRef() override { return ++refs; }
  ULONG STDMETHODCALLTYPE Release() override { const auto n = --refs; if (!n) delete this; return n; }
  HRESULT STDMETHODCALLTYPE OnReadSample(HRESULT hr, DWORD, DWORD flags, LONGLONG stamp, IMFSample* input) override {
    Sample value; value.hr = hr; value.flags = flags; value.timestamp = stamp;
    if (SUCCEEDED(hr) && input) {
      ComPtr<IMFMediaBuffer> buffer;
      if (SUCCEEDED(input->ConvertToContiguousBuffer(&buffer))) {
        BYTE* data{}; DWORD bytes{};
        if (SUCCEEDED(buffer->Lock(&data, nullptr, &bytes))) {
          if (bytes >= 4 && bytes <= maxFrame && data[0] == 0xff && data[1] == 0xd8) value.bytes.assign(data, data + bytes);
          buffer->Unlock();
        }
      }
    }
    { std::scoped_lock guard(lock); sample = std::move(value); }
    SetEvent(ready); return S_OK;
  }
  HRESULT STDMETHODCALLTYPE OnEvent(DWORD, IMFMediaEvent*) override { return S_OK; }
  HRESULT STDMETHODCALLTYPE OnFlush(DWORD) override { return S_OK; }
  Sample Take() { std::scoped_lock guard(lock); Sample value = std::move(sample); sample = {}; return value; }
};
bool StopRequested() {
  DWORD available{}; HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
  if (!PeekNamedPipe(input, nullptr, 0, nullptr, &available, nullptr)) return true;
  if (available) { BYTE byte{}; DWORD got{}; ReadFile(input, &byte, 1, &got, nullptr); return true; }
  return false;
}
int Capture(const Device& device, UINT32 width, UINT32 height, double fps) {
  ComPtr<IMFMediaSource> source; ComPtr<IMFSourceReader> reader; ComPtr<IMFAttributes> attributes; ComPtr<Callback> callback;
  HRESULT hr = device.activation->ActivateObject(IID_PPV_ARGS(&source));
  if (FAILED(hr)) { std::fprintf(stderr, "activate:%08lX\n", static_cast<unsigned long>(hr)); return 3; }
  callback.Attach(new Callback());
  if (!callback->ready || FAILED(MFCreateAttributes(&attributes, 1)) || FAILED(attributes->SetUnknown(MF_SOURCE_READER_ASYNC_CALLBACK, callback.Get())) ||
      FAILED(MFCreateSourceReaderFromMediaSource(source.Get(), attributes.Get(), &reader))) { source->Shutdown(); return 3; }
  hr = reader->SetStreamSelection(static_cast<DWORD>(MF_SOURCE_READER_ALL_STREAMS), FALSE);
  if (SUCCEEDED(hr)) hr = reader->SetStreamSelection(video, TRUE);
  UINT32 actualW{}, actualH{}, actualN{}, actualD{}; bool found = false;
  for (DWORD index = 0; SUCCEEDED(hr); index++) {
    ComPtr<IMFMediaType> type; GUID subtype{}; UINT32 w{}, h{}, n{}, d{};
    hr = reader->GetNativeMediaType(video, index, &type); if (FAILED(hr)) break;
    if (FAILED(type->GetGUID(MF_MT_SUBTYPE, &subtype)) || subtype != MFVideoFormat_MJPG ||
        FAILED(MFGetAttributeSize(type.Get(), MF_MT_FRAME_SIZE, &w, &h)) || FAILED(MFGetAttributeRatio(type.Get(), MF_MT_FRAME_RATE, &n, &d)) || !d ||
        (width && width != w) || (height && height != h) || (fps && (static_cast<double>(n) / d - fps > 0.01 || fps - static_cast<double>(n) / d > 0.01))) continue;
    hr = reader->SetCurrentMediaType(video, nullptr, type.Get());
    if (SUCCEEDED(hr)) {
      ComPtr<IMFMediaType> checked; GUID checkedSubtype{};
      hr = reader->GetCurrentMediaType(video, &checked);
      if (SUCCEEDED(hr) && SUCCEEDED(checked->GetGUID(MF_MT_SUBTYPE, &checkedSubtype)) && checkedSubtype == MFVideoFormat_MJPG &&
          SUCCEEDED(MFGetAttributeSize(checked.Get(), MF_MT_FRAME_SIZE, &actualW, &actualH)) &&
          SUCCEEDED(MFGetAttributeRatio(checked.Get(), MF_MT_FRAME_RATE, &actualN, &actualD)) && actualW == w && actualH == h && actualN == n && actualD == d) found = true;
    }
    break;
  }
  int result = 3;
  if (found) {
    std::fprintf(stderr, "mode:%u,%u,%u,%u\n", actualW, actualH, actualN, actualD);
    const auto deadline = GetTickCount64() + 5000; bool received = false; result = 0;
    hr = reader->ReadSample(video, 0, nullptr, nullptr, nullptr, nullptr);
    while (SUCCEEDED(hr) && !StopRequested()) {
      if (!received && GetTickCount64() >= deadline) { std::fprintf(stderr, "no-first-frame\n"); result = 2; break; }
      if (WaitForSingleObject(callback->ready, 100) != WAIT_OBJECT_0) continue;
      auto sample = callback->Take();
      if (FAILED(sample.hr) || (sample.flags & MF_SOURCE_READERF_ERROR)) { result = 3; break; }
      if (!sample.bytes.empty()) {
        // Eight little-endian uint32 words, then exactly jpeg length bytes.
        const auto stamp = static_cast<uint64_t>(sample.timestamp);
        const uint32_t header[8] = {0x4d4a5047, static_cast<uint32_t>(sample.bytes.size()), actualW, actualH, actualN, actualD,
          static_cast<uint32_t>(stamp), static_cast<uint32_t>(stamp >> 32)};
        if (std::fwrite(header, sizeof(header), 1, stdout) != 1 || std::fwrite(sample.bytes.data(), 1, sample.bytes.size(), stdout) != sample.bytes.size() || std::fflush(stdout)) { result = 3; break; }
        received = true;
      }
      hr = reader->ReadSample(video, 0, nullptr, nullptr, nullptr, nullptr);
    }
    if (FAILED(hr)) result = 3;
  } else std::fprintf(stderr, "unsupported-media-type:%08lX\n", static_cast<unsigned long>(hr));
  reader->Flush(video); source->Shutdown(); reader.Reset(); device.activation->ShutdownObject(); return result;
}
int wmain(int argc, wchar_t** argv) {
  if (FAILED(CoInitializeEx(nullptr, COINIT_MULTITHREADED))) return 4;
  if (FAILED(MFStartup(MF_VERSION))) { CoUninitialize(); return 4; }
  _setmode(_fileno(stdout), _O_BINARY); int result = 4;
  {
    auto devices = Devices();
    if (argc == 2 && wcscmp(argv[1], L"--list") == 0) {
      std::printf("["); bool first = true;
      for (const auto& d : devices) { std::printf("%s{\"root\":%s,\"link\":%s}", first ? "" : ",", Json(d.root).c_str(), Json(d.link).c_str()); first = false; }
      std::printf("]\n"); result = 0;
    } else if (argc == 5) {
      const std::wstring root = Lower(argv[1]); UINT32 w = static_cast<UINT32>(_wtoi(argv[2])), h = static_cast<UINT32>(_wtoi(argv[3])); double fps = _wtof(argv[4]);
      std::vector<size_t> matches;
      for (size_t i = 0; i < devices.size(); i++) if (devices[i].root == root) matches.push_back(i);
      if (matches.size() == 1 && w <= 8192 && h <= 8192 && fps <= 240) result = Capture(devices[matches[0]], w, h, fps);
      else std::fprintf(stderr, "exact-root-match-count:%zu\n", matches.size());
    }
  }
  MFShutdown(); CoUninitialize(); return result;
}
