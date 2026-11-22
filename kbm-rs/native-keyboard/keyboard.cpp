#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <array>
#include <atomic>
#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <iostream>
#include <string>
#include <thread>
#include <new>

struct Event { char kind; unsigned generation, usage, down; };
static std::array<Event, 512> events;
static std::atomic<unsigned> written{0}, consumed{0};
static std::atomic<bool> failed{false};
static bool enabled = false;
static unsigned generation = 0;
struct Held { unsigned usage = 0; bool active = false; bool host = false; bool pulse = false; };
static std::array<Held, 768> held;
static HWND target = nullptr;
static DWORD messageThread = 0;
static std::atomic<unsigned long long> callbacks{0}, swallowed{0};
static std::atomic<bool> isForeground{false};
static std::atomic<unsigned> wanted{0}, armed{0}, nativeError{0};
static std::atomic<ULONGLONG> leaseTouch{0};
static std::atomic<bool> hookInstalled{false};
static std::atomic<unsigned long long> windowCallbacks{0}, windowSwallowed{0};
static std::atomic<unsigned> windowInFlight{0};
// Optional bounded, transient diagnostic counters for an explicitly started
// Win-key trial. No text, arbitrary key identities or input device paths.
struct InputTrace { unsigned version, size; std::atomic<unsigned long long> value[18]; };
static InputTrace* inputTrace = nullptr;
static HANDLE traceMapping = nullptr;
static void TraceCount(unsigned at) { if (inputTrace) ++inputTrace->value[at]; }
[[maybe_unused]] static void TraceValue(unsigned at, unsigned long long value) { if (inputTrace) inputTrace->value[at] = value; }
static constexpr ULONG_PTR kHostMaskTag = 0x4b424d4dU;
static std::atomic<unsigned> maskedWin{0};
static std::array<std::atomic<unsigned>,2> localWinUp{};
static std::array<INPUT,3> HostMaskPlan(unsigned vk) {
  std::array<INPUT,3> plan{};
  for (auto &event:plan) {event.type=INPUT_KEYBOARD;event.ki.dwExtraInfo=kHostMaskTag;}
  plan[0].ki.wVk=0xe8; // Unassigned menu-mask key, not a text or shortcut key.
  plan[1].ki.wVk=0xe8;plan[1].ki.dwFlags=KEYEVENTF_KEYUP;
  plan[2].ki.wVk=static_cast<WORD>(vk);plan[2].ki.dwFlags=KEYEVENTF_KEYUP|KEYEVENTF_EXTENDEDKEY;
  return plan;
}
static SRWLOCK policyLock = SRWLOCK_INIT;
struct PolicyGuard {
  PolicyGuard() { AcquireSRWLockExclusive(&policyLock); }
  ~PolicyGuard() { ReleaseSRWLockExclusive(&policyLock); }
};
static void FailQueue() {
  failed = true; enabled = false; armed = 0;
  if (messageThread) PostThreadMessageW(messageThread, WM_QUIT, 0, 0);
}

static void Emit(char kind, unsigned usage = 0, unsigned down = 0)
{
  if (kind == 'K' && (usage == 0xe3 || usage == 0xe7)) TraceCount(down ? 6 : 7);
  const unsigned w = written.load(std::memory_order_relaxed);
  if (w - consumed.load(std::memory_order_acquire) == events.size()) {
    FailQueue(); return;
  }
  events[w % events.size()] = {kind, generation, usage, down};
  written.store(w + 1, std::memory_order_release);
}

static unsigned Usage(DWORD vk, DWORD scan, bool extended)
{
  if (vk == VK_HANGUL) return 0x90;
  if (vk == VK_HANJA) return 0x91;
  if (vk == VK_PAUSE) return 0x48;
  if (vk == VK_SNAPSHOT) return 0x46;
  if (vk >= VK_F13 && vk <= VK_F24) return 0x68 + vk - VK_F13;
  if (vk == VK_VOLUME_MUTE) return 0x7f;
  if (vk == VK_VOLUME_UP) return 0x80;
  if (vk == VK_VOLUME_DOWN) return 0x81;
  if (scan == 0) {
    if (vk >= 'A' && vk <= 'Z') return 4 + vk - 'A';
    switch (vk) {
      case VK_LWIN: return 0xe3; case VK_RWIN: return 0xe7;
      case VK_LSHIFT: return 0xe1; case VK_RSHIFT: return 0xe5;
      case VK_CONTROL: case VK_LCONTROL: return 0xe0; case VK_RCONTROL: return 0xe4;
      case VK_MENU: case VK_LMENU: return 0xe2; case VK_RMENU: return 0xe6;
      default: return 0;
    }
  }
  if (extended) {
    switch (scan) {
      case 0x1c: return 0x58; case 0x1d: return 0xe4;
      case 0x35: return 0x54; case 0x37: return 0x46; case 0x38: return 0xe6;
      case 0x47: return 0x4a; case 0x48: return 0x52; case 0x49: return 0x4b;
      case 0x4b: return 0x50; case 0x4d: return 0x4f; case 0x4f: return 0x4d;
      case 0x50: return 0x51; case 0x51: return 0x4e; case 0x52: return 0x49;
      case 0x53: return 0x4c; case 0x5b: return 0xe3; case 0x5c: return 0xe7;
      case 0x5d: return 0x65; default: return 0;
    }
  }
  if (scan == 0x73) return 0x87;
  if (scan == 0x7d) return 0x89;
  if (scan == 0x72 || scan == 0xf2) return 0x90;
  if (scan == 0x71 || scan == 0xf1) return 0x91;
  static constexpr unsigned char set1[] = {
    0,0x29,0x1e,0x1f,0x20,0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x2d,0x2e,0x2a,0x2b,
    0x14,0x1a,0x08,0x15,0x17,0x1c,0x18,0x0c,0x12,0x13,0x2f,0x30,0x28,0xe0,0x04,0x16,
    0x07,0x09,0x0a,0x0b,0x0d,0x0e,0x0f,0x33,0x34,0x35,0xe1,0x31,0x1d,0x1b,0x06,0x19,
    0x05,0x11,0x10,0x36,0x37,0x38,0xe5,0x55,0xe2,0x2c,0x39,0x3a,0x3b,0x3c,0x3d,0x3e,
    0x3f,0x40,0x41,0x42,0x43,0x53,0x47,0x5f,0x60,0x61,0x56,0x5c,0x5d,0x5e,0x57,0x59,
    0x5a,0x5b,0x62,0x63,0,0,0x64,0x44,0x45
  };
  return scan < sizeof(set1) ? set1[scan] : 0;
}

static void Reset()
{
  if (enabled) Emit('R');
  enabled = false;
  armed = 0;
  maskedWin=0;
  for (auto &key : held) key.active = false;
}

static void SnapshotHostKeys()
{
  for (auto &key : held) if (key.host) key = {};
  for (unsigned vk = 0; vk < 256; ++vk) {
    if (vk == VK_SHIFT || vk == VK_CONTROL || vk == VK_MENU || GetAsyncKeyState(static_cast<int>(vk)) >= 0) continue;
    unsigned scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX);
    if (vk == VK_LSHIFT) scan = 0x2a;
    if (vk == VK_RSHIFT) scan = 0x36;
    if (vk == VK_LCONTROL) scan = 0x1d;
    if (vk == VK_RCONTROL) scan = 0xe01d;
    if (vk == VK_LMENU) scan = 0x38;
    if (vk == VK_RMENU) scan = 0xe038;
    const unsigned token = (scan & 255U) == 0 ? 512 + vk : (scan & 255U) + ((scan & 0xff00U) != 0 ? 256U : 0U);
    if (held[token].usage == 0) held[token].host = true;
  }
}

// Pure routing policy is shared by the hook and --self-test; no key injection.
static bool Route(DWORD vk, DWORD scan, DWORD flags, bool down, bool foreground, bool freshLanguage = false)
{
  if (scan >= 256 || vk >= 256) return false;
  const unsigned token = scan == 0 ? 512 + vk : scan + ((flags & LLKHF_EXTENDED) != 0 ? 256U : 0U);
  Held &key = held[token];
  if (!foreground) Reset();
  if (key.host) { if (!down) key = {}; return false; }
  const unsigned mapped = Usage(vk, scan, (flags & LLKHF_EXTENDED) != 0);
  const bool language = mapped == 0x90 || mapped == 0x91;
  const bool dedicatedLanguage = language && (scan == 0 || (scan & 0x7fU) == 0x71 || (scan & 0x7fU) == 0x72);
  if (dedicatedLanguage && enabled && foreground) {
    // Windows LANG1/2 scan codes (71/72, F1/F2) can represent a complete
    // release-only stroke. Their previous-state bit is not a reliable make/
    // break pairing. Every standalone stroke must remain usable after the first.
    // If a driver does provide DOWN+UP, its UP only clears the matching marker.
    if (!down && key.pulse) { key = {}; return true; }
    key = down ? Held{mapped, false, false, true} : Held{};
    Emit('K', mapped, 1); Emit('K', mapped, 0); return true;
  }
  // Some Korean layouts do not provide a matching UP. A new (non-repeat)
  // message starts a fresh pulse even if the preceding UP was absent.
  if (language && down && freshLanguage && key.pulse) key = {};
  if (!language && down && freshLanguage && enabled && foreground && key.usage != 0 && !key.active) {
    // A genuine new press after blur must not remain trapped behind the old
    // suppressed-UP tombstone. Without a new DOWN, old UP stays cleanup-only.
    key = {};
  }
  // Once a new DOWN is passed to another window, its UP belongs to that host.
  // An old suppressed marker must never swallow the UP of a host-owned DOWN.
  if (down && (!foreground || !enabled) && key.usage != 0) {
    key = {}; key.host = true; return false;
  }
  if (!down && key.usage != 0) {
    if (key.pulse && enabled && foreground) { key = {}; return true; }
    if (key.active && enabled) Emit('K', key.usage, 0);
    else {
      const unsigned w = written.load(std::memory_order_relaxed);
      if (w - consumed.load(std::memory_order_acquire) == events.size()) { FailQueue(); return true; }
      events[w % events.size()] = {'U', vk, scan, (flags & LLKHF_EXTENDED) != 0 ? 1U : 0U};
      written.store(w + 1, std::memory_order_release);
    }
    key = {}; return true; // Swallow the UP paired with a suppressed host DOWN.
  }
  if (!enabled || !foreground) return false;
  if (down && key.usage != 0) return true; // Target OS owns key repeat.
  if (!down && !language) return false;
  const unsigned usage = mapped;
  if (usage == 0) return false;
  if (language) {
    // IME toggle is atomic: never leave LANG1/2 held on the target, even for
    // DOWN-only or UP-only layouts. A matching UP only clears the marker.
    key = down ? Held{usage, false, false, true} : Held{};
    Emit('K', usage, 1); Emit('K', usage, 0); return true;
  }
  key = {usage, true}; Emit('K', usage, 1); return true;
}

static LRESULT CALLBACK Keyboard(int code, WPARAM message, LPARAM data)
{
#ifdef MINIKMB_KEYBOARD_DLL
  struct Flight { Flight() { ++windowInFlight; } ~Flight() { --windowInFlight; } } flight;
#endif
  callbacks.fetch_add(1, std::memory_order_relaxed);
  if (code == HC_ACTION && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN || message == WM_KEYUP || message == WM_SYSKEYUP)) {
    const auto *key = reinterpret_cast<const KBDLLHOOKSTRUCT *>(data);
    // The local menu mask and local Win release must reach Windows, never the
    // target keyboard or its held-key policy.
    if (key->dwExtraInfo==kHostMaskTag) return CallNextHookEx(nullptr,code,message,data);
    if (key->vkCode == VK_LWIN || key->vkCode == VK_RWIN) TraceCount((message == WM_KEYDOWN || message == WM_SYSKEYDOWN) ? 0 : 1);
    // Language toggles use the GUI hook's previous-state bit to distinguish a
    // new press from auto-repeat. Do not feed this same key through both hooks.
#ifdef MINIKMB_KEYBOARD_DLL
    const unsigned usage = Usage(key->vkCode, key->scanCode, (key->flags & LLKHF_EXTENDED) != 0);
    if (usage == 0x90 || usage == 0x91) return CallNextHookEx(nullptr, code, message, data);
#endif
    bool blocked = false;
    {
    PolicyGuard guard;
    const bool foreground = GetForegroundWindow() == target;
    isForeground.store(foreground, std::memory_order_relaxed);
    bool ctrl = false, alt = false;
    for (const auto &heldKey : held) {
      if (heldKey.active && (heldKey.usage == 0xe0 || heldKey.usage == 0xe4)) ctrl = true;
      if (heldKey.active && (heldKey.usage == 0xe2 || heldKey.usage == 0xe6)) alt = true;
    }
    const bool secure = key->vkCode == VK_DELETE && ((ctrl && alt) || (GetAsyncKeyState(VK_CONTROL) < 0 && GetAsyncKeyState(VK_MENU) < 0));
    if (secure) Reset();
    const bool scope = foreground
#ifdef MINIKMB_KEYBOARD_DLL
      && wanted.load(std::memory_order_acquire) == generation
      && GetTickCount64() - leaseTouch.load(std::memory_order_acquire) <= 750
#endif
      ;
    if (!secure) blocked = Route(key->vkCode, key->scanCode, key->flags, message == WM_KEYDOWN || message == WM_SYSKEYDOWN, scope);
    }
    if (blocked) { swallowed.fetch_add(1, std::memory_order_relaxed); return 1; }
  }
  return CallNextHookEx(nullptr, code, message, data);
}

#ifdef MINIKMB_KEYBOARD_DLL
struct WindowFlight { WindowFlight() { ++windowInFlight; } ~WindowFlight() { --windowInFlight; } };
static thread_local bool keyboardMessageSeen = false;
static thread_local bool keyboardMessageBlocked = false;
static thread_local WPARAM keyboardMessageVk = 0;
static thread_local LPARAM keyboardMessageBits = 0;
static bool RouteWindow(WPARAM vk, LPARAM bits) {
  if ((vk==VK_LWIN||vk==VK_RWIN) && (bits&(1LL<<31))!=0) {
    const unsigned index=vk==VK_LWIN?0U:1U;
    const unsigned pending=localWinUp[index].load();
    if (static_cast<ULONG_PTR>(GetMessageExtraInfo())==kHostMaskTag || pending!=0) {
      if (pending!=0) --localWinUp[index];
      return false;
    }
  }
  bool blocked = false;
  {
    PolicyGuard guard;
    const bool foreground = GetForegroundWindow() == target;
    const bool scope = foreground && wanted.load() == generation && GetTickCount64() - leaseTouch.load() <= 750;
    bool ctrl = false, alt = false;
    for (const auto &key : held) {
      if (key.active && (key.usage == 0xe0 || key.usage == 0xe4)) ctrl = true;
      if (key.active && (key.usage == 0xe2 || key.usage == 0xe6)) alt = true;
    }
    const bool secure = vk == VK_DELETE && ((ctrl && alt) || (GetAsyncKeyState(VK_CONTROL)<0 && GetAsyncKeyState(VK_MENU)<0));
    if (secure) Reset();
    else blocked = Route(static_cast<DWORD>(vk), static_cast<DWORD>((static_cast<ULONG_PTR>(bits)>>16)&255U),
      (bits&(1LL<<24)) ? LLKHF_EXTENDED : 0, (bits&(1LL<<31))==0, scope, (bits&(1LL<<30))==0);
  }
  return blocked;
}
static void MaskWindowWin(WPARAM vk, LPARAM bits, bool blocked) {
  if (!blocked || (vk!=VK_LWIN&&vk!=VK_RWIN) || (bits&((1LL<<31)|(1LL<<30)))!=0
      || GetForegroundWindow()!=target || wanted.load()==0 || armed.load()!=wanted.load()
      || GetTickCount64()-leaseTouch.load()>750) return;
  const unsigned index=vk==VK_LWIN?0U:1U;
  auto plan=HostMaskPlan(static_cast<unsigned>(vk));
  ++localWinUp[index];maskedWin.fetch_or(1U<<index);TraceCount(16);
  // Do not hold policyLock across SendInput: the low-level hook may re-enter.
  const UINT sent=SendInput(static_cast<UINT>(plan.size()),plan.data(),sizeof(INPUT));
  if (sent!=plan.size()) {
    const DWORD error=GetLastError();TraceCount(17);localWinUp[index]=0;maskedWin.fetch_and(~(1U<<index));
    if (sent==1) SendInput(1,&plan[1],sizeof(INPUT)); // Release an accepted mask DOWN only.
    {PolicyGuard guard;nativeError=error;wanted=0;Reset();failed=true;}
    PostThreadMessageW(messageThread,WM_QUIT,0,0);
  }
}
static LRESULT CALLBACK WindowKeyboard(int code, WPARAM vk, LPARAM bits)
{
  WindowFlight flight;
  bool blocked = false;
  if (code == HC_ACTION) {
    windowCallbacks.fetch_add(1, std::memory_order_relaxed);
    blocked = RouteWindow(vk,bits);
    MaskWindowWin(vk,bits,blocked);
    keyboardMessageSeen = true; keyboardMessageBlocked = blocked; keyboardMessageVk = vk; keyboardMessageBits = bits;
  }
  if (blocked) { windowSwallowed.fetch_add(1, std::memory_order_relaxed); return 1; }
  return CallNextHookEx(nullptr, code, vk, bits);
}
// Posted key messages do not invoke WH_KEYBOARD. Keep this queue hook as the
// scoped fallback; it also prevents a second pass after a physical key hook.
static LRESULT CALLBACK QueueMessage(int code, WPARAM removed, LPARAM data)
{
  WindowFlight flight;
  if (code == HC_ACTION && removed == PM_REMOVE) {
    auto *msg = reinterpret_cast<MSG*>(data);
    const bool seen = keyboardMessageSeen && keyboardMessageVk == msg->wParam && keyboardMessageBits == msg->lParam;
    const bool blocked = seen && keyboardMessageBlocked;
    keyboardMessageSeen = false;
    if ((inputTrace || maskedWin.load()!=0) && msg->message == WM_INPUT) {
      RAWINPUT raw{}; UINT size = sizeof(raw);
      if (GetRawInputData(reinterpret_cast<HRAWINPUT>(msg->lParam),RID_INPUT,&raw,&size,sizeof(RAWINPUTHEADER)) != UINT(-1)
          && raw.header.dwType == RIM_TYPEKEYBOARD && raw.data.keyboard.ExtraInformation!=kHostMaskTag
          && (raw.data.keyboard.VKey == VK_LWIN || raw.data.keyboard.VKey == VK_RWIN)) {
        TraceCount((raw.data.keyboard.Flags & RI_KEY_BREAK) ? 5 : 4);
        TraceValue(11,raw.data.keyboard.Flags);
        wchar_t device[512]{};UINT chars=512;
        if (inputTrace && GetRawInputDeviceInfoW(raw.header.hDevice,RIDI_DEVICENAME,device,&chars)!=UINT(-1)) {
          device[511]=0;CharUpperBuffW(device,static_cast<DWORD>(wcslen(device)));
          TraceValue(12,wcsstr(device,L"VID_CAFE") ? 2 : 1);
        }
        if ((raw.data.keyboard.Flags&RI_KEY_BREAK)!=0) {
          const unsigned index=raw.data.keyboard.VKey==VK_LWIN?0U:1U;
          if ((maskedWin.fetch_and(~(1U<<index))&(1U<<index))!=0) {
            localWinUp[index]=0;
            PolicyGuard guard;
            const bool scope=GetForegroundWindow()==target && wanted.load()==generation && GetTickCount64()-leaseTouch.load()<=750;
            Route(raw.data.keyboard.VKey,raw.data.keyboard.MakeCode,(raw.data.keyboard.Flags&RI_KEY_E0)?LLKHF_EXTENDED:0,false,scope);
          }
        }
      }
    }
    if (msg->hwnd == target && (msg->message == WM_KEYDOWN || msg->message == WM_KEYUP || msg->message == WM_SYSKEYDOWN || msg->message == WM_SYSKEYUP)) {
      if (msg->wParam == VK_LWIN || msg->wParam == VK_RWIN) {
        TraceCount((msg->message == WM_KEYDOWN || msg->message == WM_SYSKEYDOWN) ? 2 : 3);
        TraceValue(8,static_cast<ULONG_PTR>(msg->lParam));TraceValue(15,GetTickCount64());
        INPUT_MESSAGE_SOURCE source{};
        if (GetCurrentInputMessageSource(&source)) {TraceValue(9,source.originId);TraceValue(10,source.deviceType);}
      }
      windowCallbacks.fetch_add(1, std::memory_order_relaxed);
      const bool routed = !seen && RouteWindow(msg->wParam,msg->lParam);
      if (blocked || routed) {
        if (!seen) MaskWindowWin(msg->wParam,msg->lParam,routed);
        windowSwallowed.fetch_add(1, std::memory_order_relaxed);
        msg->message = WM_NULL; msg->wParam = 0; msg->lParam = 0;
      }
    }
  }
  return CallNextHookEx(nullptr,code,removed,data);
}
#endif

#ifndef MINIKMB_KEYBOARD_DLL
static int SelfTest()
{
  assert(Usage(VK_HANGUL, 0x38, true) == 0x90);
  assert(Usage(VK_HANJA, 0x1d, true) == 0x91);
  assert(Usage(VK_LWIN, 0x5b, true) == 0xe3);
  assert(Usage(VK_TAB, 0x0f, false) == 0x2b);
  assert(!Route('A', 0x1e, 0, true, true));
  enabled = true; generation = 1;
  assert(Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, true, true));
  assert(Route('R', 0x13, 0, true, true));
  assert(Route('R', 0x13, 0, false, true));
  assert(Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, false, true));
  assert(written.load() == 4);
  assert(events[0].usage == 0xe3 && events[1].usage == 0x15 && events[3].down == 0);
  assert(Route(VK_HANGUL, 0x38, LLKHF_EXTENDED, true, true));
  assert(Route(VK_RMENU, 0x38, LLKHF_EXTENDED, false, true));
  assert(events[5].usage == 0x90); // UP keeps original usage even when IME changes VK.
  assert(Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, true, true));
  assert(!Route('B', 0x30, 0, true, false));
  assert(!enabled && events[7].kind == 'R');
  assert(Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, false, false));
  assert(!Route('A', 0x1e, LLKHF_INJECTED, true, false));
  enabled = true;
  assert(Route(VK_LCONTROL, 0x1d, LLKHF_INJECTED, true, true));
  assert(Route(VK_LCONTROL, 0x1d, LLKHF_INJECTED, false, true));
  assert(Usage(0, 0x73, false) == 0x87);
  assert(Usage(0, 0x7d, false) == 0x89);
  assert(Usage(VK_RWIN, 0, false) == 0xe7);
  Reset(); held = {}; enabled = true; generation = 2;
  held[0x1d].host = true;
  assert(!Route(VK_LCONTROL, 0x1d, 0, true, true));
  assert(!Route(VK_LCONTROL, 0x1d, 0, false, true));
  assert(Route(VK_LCONTROL, 0x1d, 0, true, true));
  Reset(); generation = 3; enabled = true;
  const unsigned beforeOldUp = written.load();
  assert(Route(VK_LCONTROL, 0x1d, 0, false, true));
  assert(events[beforeOldUp % events.size()].kind == 'U');
  held = {}; enabled = true;
  assert(Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, true, true));
  assert(!Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, true, false));
  assert(!Route(VK_LWIN, 0x5b, LLKHF_EXTENDED, false, false));
  assert(!Route('A', 0x1e, 0, true, false));
  assert(!Route('A', 0x1e, 0, false, false));
  held = {}; enabled = true; generation = 4;
  unsigned pulse = written.load();
  assert(Route(VK_HANGUL, 0x38, LLKHF_EXTENDED, true, true, true));
  assert(Route(VK_HANGUL, 0x38, LLKHF_EXTENDED, true, true, false));
  assert(written.load() == pulse + 2); // Auto-repeat does not toggle again.
  assert(events[pulse % events.size()].down == 1 && events[(pulse+1) % events.size()].down == 0);
  assert(Route(VK_RMENU, 0x38, LLKHF_EXTENDED, false, true));
  assert(written.load() == pulse + 2); // Matching UP only clears the pulse.
  assert(Route(VK_HANGUL, 0x38, LLKHF_EXTENDED, true, true, true));
  assert(Route(VK_HANGUL, 0x38, LLKHF_EXTENDED, true, true, true));
  assert(written.load() == pulse + 6); // Two real new presses, even without UP.
  held = {};
  assert(Route(VK_HANJA, 0x71, 0, false, true));
  assert(written.load() == pulse + 8); // UP-only layouts also end released.
  for (const unsigned scan : {0U,0x71U,0x72U,0xf1U,0xf2U}) {
    for (const bool extended : {false,true}) {
      for (const bool down : {false,true}) {
        held = {}; enabled = true;
        const unsigned before = written.load();
        const unsigned vk = (scan & 0x7fU) == 0x71 ? VK_HANJA : VK_HANGUL;
        const unsigned usage = vk == VK_HANJA ? 0x91U : 0x90U;
        for (unsigned press=0;press<3;++press) {
          // Reproduce the broken case: no paired edge and previous-state=1
          // for every independent stroke, including the second and third.
          assert(Route(vk,scan,extended?LLKHF_EXTENDED:0,down,true,false));
          assert(written.load()==before+2*(press+1));
          const unsigned at=(before+2*press)%events.size();
          assert(events[at].usage==usage && events[at].down==1);
          assert(events[(at+1)%events.size()].usage==usage && events[(at+1)%events.size()].down==0);
        }
      }
    }
  }
  held = {};enabled=true;generation=8;
  const unsigned winBefore=written.load();
  assert(Route(VK_LWIN,0x5b,LLKHF_EXTENDED,true,true,true));
  assert(!Route('A',0x1e,0,true,false)); // blur; the Win UP is not observed
  enabled=true;generation=9;
  assert(Route(VK_LWIN,0x5b,LLKHF_EXTENDED,true,true,true));
  assert(Route(VK_LWIN,0x5b,LLKHF_EXTENDED,false,true));
  assert(written.load()==winBefore+4); // DOWN, release owner, new DOWN/UP
  for (const unsigned vk : {static_cast<unsigned>(VK_LWIN),static_cast<unsigned>(VK_RWIN)}) {
    const auto plan=HostMaskPlan(vk);
    assert(plan[0].ki.wVk==0xe8 && plan[0].ki.dwFlags==0);
    assert(plan[1].ki.wVk==0xe8 && plan[1].ki.dwFlags==KEYEVENTF_KEYUP);
    assert(plan[2].ki.wVk==vk && plan[2].ki.dwFlags==(KEYEVENTF_KEYUP|KEYEVENTF_EXTENDEDKEY));
    for (const auto &event:plan) assert(event.type==INPUT_KEYBOARD && event.ki.dwExtraInfo==kHostMaskTag);
  } // Build/inspect only: self-tests never call SendInput.
  puts("PASS keyboard native mappings, host suppression, Win+R, IME UP, foreground loss, paired cleanup and scoped synthetic modifiers; no hook installed");
  return 0;
}
#endif

#ifdef MINIKMB_KEYBOARD_DLL
static std::thread* nativeThread = nullptr;
static HANDLE readyEvent = nullptr, exitedEvent = nullptr;
static std::atomic<bool> testMode{false};
static DWORD nativeMessageThread = 0;
static DWORD keyboardThread = 0;
static HHOOK lowLevelHook = nullptr;
static std::atomic<unsigned> hookEpoch{0};
static bool ReinstallLowLevelHook() {
  if (lowLevelHook) {
    if (!UnhookWindowsHookEx(lowLevelHook) && GetLastError()!=ERROR_INVALID_HOOK_HANDLE) return false;
    lowLevelHook=nullptr;
  }
  HMODULE module=nullptr;
  if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
      reinterpret_cast<LPCWSTR>(&Keyboard),&module)) return false;
  lowLevelHook=SetWindowsHookExW(WH_KEYBOARD_LL,Keyboard,module,0);
  if (lowLevelHook) {TraceCount(13);TraceValue(14,GetTickCount64());}
  return lowLevelHook!=nullptr;
}

struct NativeStats {
  unsigned size, flags, error, threadId, generation, desired;
  unsigned long long callbacks, swallowed;
  unsigned long long windowCallbacks, windowSwallowed;
};
static_assert(sizeof(Event) == 16 && sizeof(NativeStats) == 56, "FFI layout changed");

extern "C" __declspec(dllexport) unsigned __cdecl kbm_stop();
extern "C" __declspec(dllexport) unsigned __cdecl kbm_start(uintptr_t window, unsigned parentPid, unsigned testing)
{
  if (nativeThread != nullptr) return 2;
  DWORD pid = 0;
  target = reinterpret_cast<HWND>(window);
  keyboardThread = GetWindowThreadProcessId(target, &pid);
  if (!IsWindow(target) || pid != parentPid || pid != GetCurrentProcessId() || keyboardThread != GetCurrentThreadId()) return 0;
  written = 0; consumed = 0; failed = false; enabled = false; generation = 0;
  held = {}; callbacks = 0; swallowed = 0; windowCallbacks = 0; windowSwallowed = 0; wanted = 0; armed = 0; nativeError = 0;
  hookInstalled = false; isForeground = false; testMode = testing != 0;
  hookEpoch=0;
  wchar_t traceEnabled[2]{};
  if (!testMode && GetEnvironmentVariableW(L"MINIKMB_KEYBOARD_TRACE",traceEnabled,2)==1 && traceEnabled[0]==L'1') {
    wchar_t name[96]{};swprintf_s(name,L"Local\\MiniKMBKeyboardTrace-%lu",GetCurrentProcessId());
    traceMapping=CreateFileMappingW(INVALID_HANDLE_VALUE,nullptr,PAGE_READWRITE,0,sizeof(InputTrace),name);
    if (traceMapping) {
      void* view=MapViewOfFile(traceMapping,FILE_MAP_WRITE,0,0,sizeof(InputTrace));
      if (view) {inputTrace=new(view) InputTrace{};inputTrace->version=1;inputTrace->size=sizeof(InputTrace);}
    }
  }
  readyEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  exitedEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
  if (!readyEvent || !exitedEvent) { kbm_stop(); return 0; }
  try { nativeThread = new std::thread([] {
    nativeMessageThread = GetCurrentThreadId();
    messageThread = nativeMessageThread;
    MSG message; PeekMessageW(&message, nullptr, WM_USER, WM_USER, PM_NOREMOVE);
    HMODULE module = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
      reinterpret_cast<LPCWSTR>(&Keyboard), &module);
    HHOOK windowHook = testMode ? nullptr : SetWindowsHookExW(WH_KEYBOARD, WindowKeyboard, module, GetWindowThreadProcessId(target, nullptr));
    HHOOK queueHook = testMode ? nullptr : SetWindowsHookExW(WH_GETMESSAGE, QueueMessage, module, GetWindowThreadProcessId(target, nullptr));
    hookInstalled = windowHook != nullptr && queueHook != nullptr;
    if (!testMode && !hookInstalled) { nativeError = GetLastError(); failed = true; }
    const UINT_PTR timer = SetTimer(nullptr, 1, 25, nullptr);
    if (!timer) { nativeError = GetLastError(); failed = true; }
    SetEvent(readyEvent);
    while (!failed && GetMessageW(&message, nullptr, 0, 0) > 0) {
      PolicyGuard guard;
      if (message.message == WM_APP + 1) {
        Reset(); generation = static_cast<unsigned>(message.lParam);
        const bool valid = message.wParam != 0 && wanted.load() == generation;
        if (valid && !testMode) SnapshotHostKeys();
        enabled = valid && (testMode || GetForegroundWindow() == target);
        armed = enabled ? generation : 0;
        Emit('S', enabled ? 1U : 0U);
      } else if (message.message == WM_APP + 2 && testMode) {
        const unsigned packed = static_cast<unsigned>(message.lParam);
        Route(static_cast<unsigned>(message.wParam), packed & 255U,
          (packed & 256U) ? LLKHF_EXTENDED : 0, (packed & 512U) != 0, (packed & 1024U) != 0);
        if (!enabled) armed = 0;
      } else if (message.message == WM_APP + 3) {
        const unsigned epoch = static_cast<unsigned>(message.lParam);
        const unsigned packed = static_cast<unsigned>(message.wParam);
        const bool scope = enabled && wanted.load() == epoch && generation == epoch
          && GetTickCount64() - leaseTouch.load() <= 750
          && (testMode || GetForegroundWindow() == target);
        const unsigned vk = packed & 255U;
        bool ctrl = false, alt = false;
        for (const auto &heldKey : held) {
          if (heldKey.active && (heldKey.usage == 0xe0 || heldKey.usage == 0xe4)) ctrl = true;
          if (heldKey.active && (heldKey.usage == 0xe2 || heldKey.usage == 0xe6)) alt = true;
        }
        if (vk == VK_DELETE && ((ctrl && alt) || (GetAsyncKeyState(VK_CONTROL)<0 && GetAsyncKeyState(VK_MENU)<0))) Reset();
        else Route(vk, (packed>>8)&255U, (packed&65536U)?LLKHF_EXTENDED:0, (packed&131072U)!=0, scope);
      } else if (message.message == WM_TIMER) {
        const bool foreground = testMode || GetForegroundWindow() == target;
        isForeground = foreground;
        if (!IsWindow(target)) break;
        if (enabled && (!foreground || wanted.load() != generation || GetTickCount64() - leaseTouch.load() > 750)) {
          Reset(); armed = 0;
        }
      }
    }
    { PolicyGuard guard; Reset(); armed = 0; }
    if (windowHook) UnhookWindowsHookEx(windowHook);
    if (queueHook) UnhookWindowsHookEx(queueHook);
    hookInstalled = false;
    if (timer) KillTimer(nullptr, timer);
    SetEvent(exitedEvent);
  }); } catch (...) { nativeError = ERROR_NOT_ENOUGH_MEMORY; failed = true; kbm_stop(); return 0; }
  if (WaitForSingleObject(readyEvent, 3000) != WAIT_OBJECT_0 || failed) { kbm_stop(); return 3; }
  if (!testMode) {
    // Install on the actual GUI input thread. Window-message suppression is
    // too late for shell Win-key handling; this callback must see and consume
    // the low-level edge before Windows dispatches it to the shell.
    if (!ReinstallLowLevelHook()) { nativeError = GetLastError(); failed = true; kbm_stop(); return 3; }
  }
  return 1;
}
extern "C" __declspec(dllexport) unsigned __cdecl kbm_enable(unsigned epoch)
{
  wanted.store(epoch, std::memory_order_release);
  if (epoch==0) hookEpoch=0;
  leaseTouch.store(GetTickCount64(), std::memory_order_release);
  return PostThreadMessageW(nativeMessageThread, WM_APP + 1, epoch != 0, epoch) ? 1U : 0U;
}
extern "C" __declspec(dllexport) void __cdecl kbm_refresh(unsigned epoch)
{
  if (epoch != 0 && wanted.load() == epoch) {
    leaseTouch.store(GetTickCount64(), std::memory_order_release);
    if (!testMode && armed.load()==epoch && hookEpoch.load()!=epoch && GetCurrentThreadId()==keyboardThread) {
      if (ReinstallLowLevelHook()) hookEpoch=epoch;
      else {nativeError=GetLastError();failed=true;wanted=0;PostThreadMessageW(nativeMessageThread,WM_QUIT,0,0);}
    }
  }
}
extern "C" __declspec(dllexport) unsigned __cdecl kbm_window_key(unsigned epoch, unsigned vk, unsigned scan, unsigned extended, unsigned down)
{
  if (epoch == 0 || wanted.load()!=epoch || vk>255 || scan>255) return 0;
  return PostThreadMessageW(nativeMessageThread, WM_APP+3,
    vk|(scan<<8)|(extended?65536U:0U)|(down?131072U:0U), epoch) ? 1U : 0U;
}
extern "C" __declspec(dllexport) unsigned __cdecl kbm_poll(Event* output, unsigned capacity)
{
  const unsigned r = consumed.load(std::memory_order_relaxed), w = written.load(std::memory_order_acquire);
  const unsigned count = (w-r < capacity) ? w-r : capacity;
  for (unsigned i = 0; i < count; ++i) output[i] = events[(r+i) % events.size()];
  consumed.store(r + count, std::memory_order_release); return count;
}
extern "C" __declspec(dllexport) void __cdecl kbm_status_v2(NativeStats* result)
{
  *result = {sizeof(NativeStats), (hookInstalled ? 1U : 0U) | (isForeground ? 2U : 0U) | (armed.load() ? 4U : 0U) | (failed ? 8U : 0U),
    nativeError.load(), keyboardThread, armed.load(), wanted.load(), callbacks.load(), swallowed.load(), windowCallbacks.load(), windowSwallowed.load()};
}
struct LegacyStats { unsigned size, flags, error, threadId, generation, desired; unsigned long long callbacks, swallowed; };
extern "C" __declspec(dllexport) void __cdecl kbm_status(LegacyStats* result)
{
  NativeStats full{}; kbm_status_v2(&full);
  *result = {sizeof(LegacyStats),full.flags,full.error,full.threadId,full.generation,full.desired,full.callbacks,full.swallowed};
}
extern "C" __declspec(dllexport) unsigned __cdecl kbm_test_route(unsigned vk, unsigned scan, unsigned extended, unsigned down, unsigned foreground)
{
  if (!testMode || scan > 255 || vk > 255) return 0;
  return PostThreadMessageW(nativeMessageThread, WM_APP + 2, vk,
    scan | (extended ? 256U : 0U) | (down ? 512U : 0U) | (foreground ? 1024U : 0U)) ? 1U : 0U;
}
extern "C" __declspec(dllexport) unsigned __cdecl kbm_stop()
{
  wanted = 0;
  bool unhooked = true;
  if (lowLevelHook) {
    unhooked = UnhookWindowsHookEx(lowLevelHook) != 0 || GetLastError() == ERROR_INVALID_HOOK_HANDLE;
    if (unhooked) lowLevelHook = nullptr;
  }
  if (nativeThread) {
    PostThreadMessageW(nativeMessageThread, WM_QUIT, 0, 0);
    if (WaitForSingleObject(exitedEvent, 2000) != WAIT_OBJECT_0) return 0;
    nativeThread->join(); delete nativeThread; nativeThread = nullptr;
  }
  const ULONGLONG deadline = GetTickCount64() + 2000;
  while (windowInFlight.load() != 0) { if (GetTickCount64() >= deadline) return 0; Sleep(0); }
  if (!unhooked) return 0; // Keep the DLL loaded if callback removal is uncertain.
  if (inputTrace) {UnmapViewOfFile(inputTrace);inputTrace=nullptr;}
  if (traceMapping) {CloseHandle(traceMapping);traceMapping=nullptr;}
  if (readyEvent) CloseHandle(readyEvent);
  if (exitedEvent) CloseHandle(exitedEvent);
  readyEvent = nullptr; exitedEvent = nullptr; return 1;
}
#else
int main(int argc, char **argv)
{
  if (argc == 2 && std::string(argv[1]) == "--self-test") return SelfTest();
  if (argc == 2 && std::string(argv[1]) == "--map-test") {
    unsigned vk, scan, extended;
    while (std::cin >> vk >> scan >> extended) std::cout << Usage(vk, scan, extended != 0) << '\n';
    return 0; // Pure mapping mode; no hook, window, or device access.
  }
  if (argc != 3) return 2;
  target = reinterpret_cast<HWND>(static_cast<uintptr_t>(std::strtoull(argv[1], nullptr, 16)));
  const DWORD parentPid = static_cast<DWORD>(std::strtoul(argv[2], nullptr, 10));
  DWORD windowPid = 0; GetWindowThreadProcessId(target, &windowPid);
  if (!IsWindow(target) || parentPid == 0 || windowPid != parentPid) return 2;
  HANDLE parent = OpenProcess(SYNCHRONIZE, FALSE, parentPid);
  if (!parent) return 2;
  messageThread = GetCurrentThreadId();
  MSG message; PeekMessageW(&message, nullptr, WM_USER, WM_USER, PM_NOREMOVE);
  HHOOK hook = SetWindowsHookExW(WH_KEYBOARD_LL, Keyboard, GetModuleHandleW(nullptr), 0);
  if (!hook) { std::printf("F %lu\n", GetLastError()); std::fflush(stdout); CloseHandle(parent); return 2; }
  std::thread([] {
    char command[34];
    while (std::cin.getline(command, sizeof(command))) {
      if (command[0] == '\0') break;
      if ((command[0] == 'E' || command[0] == 'D') && command[1] == ' ') {
        const unsigned value = static_cast<unsigned>(std::strtoul(command + 2, nullptr, 10));
        PostThreadMessageW(messageThread, WM_APP + 1, command[0] == 'E', value);
      }
    }
    PostThreadMessageW(messageThread, WM_QUIT, 0, 0);
  }).detach();
  std::thread([] {
    ULONGLONG last = GetTickCount64();
    for (;;) {
      if (GetTickCount64() - last >= 1000) {
        std::printf("T %llu %llu %u\n", callbacks.load(), swallowed.load(), isForeground.load() ? 1U : 0U);
        std::fflush(stdout); last = GetTickCount64();
      }
      const unsigned r = consumed.load(std::memory_order_relaxed);
      if (r == written.load(std::memory_order_acquire)) { Sleep(1); continue; }
      const Event e = events[r % events.size()];
      if (e.kind == 'K') std::printf("K %u %u %u\n", e.generation, e.usage, e.down);
      else if (e.kind == 'U') std::printf("U %u %u %u\n", e.generation, e.usage, e.down);
      else std::printf("%c %u %u\n", e.kind, e.generation, e.usage);
      std::fflush(stdout);
      consumed.store(r + 1, std::memory_order_release);
    }
  }).detach();
  std::puts("READY"); std::fflush(stdout);
  SetTimer(nullptr, 1, 50, nullptr);
  while (GetMessageW(&message, nullptr, 0, 0) > 0) {
    if (message.message == WM_APP + 1) {
      Reset(); generation = static_cast<unsigned>(message.lParam);
      if (message.wParam != 0) SnapshotHostKeys();
      enabled = message.wParam != 0 && GetForegroundWindow() == target;
      Emit('S', enabled ? 1U : 0U);
    } else if (message.message == WM_TIMER) {
      if (!IsWindow(target) || WaitForSingleObject(parent, 0) == WAIT_OBJECT_0) break;
      isForeground.store(GetForegroundWindow() == target, std::memory_order_relaxed);
      if (enabled && GetForegroundWindow() != target) Reset();
    }
  }
  UnhookWindowsHookEx(hook); CloseHandle(parent);
  return failed ? 3 : 0;
}
#endif
