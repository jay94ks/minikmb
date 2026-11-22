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

struct Event { char kind; unsigned generation, usage, down; };
static std::array<Event, 512> events;
static std::atomic<unsigned> written{0}, consumed{0};
static bool failed = false, enabled = false;
static unsigned generation = 0;
struct Held { unsigned usage = 0; bool active = false; };
static std::array<Held, 256> held;
static HWND target = nullptr;
static DWORD messageThread = 0;

static void Emit(char kind, unsigned usage = 0, unsigned down = 0)
{
  const unsigned w = written.load(std::memory_order_relaxed);
  if (w - consumed.load(std::memory_order_acquire) == events.size()) {
    failed = true; enabled = false; PostQuitMessage(3); return;
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
  for (auto &key : held) key.active = false;
}

// Pure routing policy is shared by the hook and --self-test; no key injection.
static bool Route(DWORD vk, DWORD scan, DWORD flags, bool down, bool foreground)
{
  const unsigned token = (scan & 0x7fU) | ((flags & LLKHF_EXTENDED) != 0 ? 0x80U : 0U);
  Held &key = held[token];
  if (!foreground) Reset();
  if (!down && key.usage != 0) {
    if (key.active && enabled) Emit('K', key.usage, 0);
    key = {}; return true; // Swallow the UP paired with a suppressed host DOWN.
  }
  if (!enabled || !foreground) return false;
  if (down && key.usage != 0) return true; // Target OS owns key repeat.
  if (!down) return false; // A key held before capture must release on the host.
  const unsigned usage = Usage(vk, scan, (flags & LLKHF_EXTENDED) != 0);
  if (usage == 0) return false;
  key = {usage, true}; Emit('K', usage, 1); return true;
}

static LRESULT CALLBACK Keyboard(int code, WPARAM message, LPARAM data)
{
  if (code == HC_ACTION && (message == WM_KEYDOWN || message == WM_SYSKEYDOWN || message == WM_KEYUP || message == WM_SYSKEYUP)) {
    const auto *key = reinterpret_cast<const KBDLLHOOKSTRUCT *>(data);
    if (Route(key->vkCode, key->scanCode, key->flags, message == WM_KEYDOWN || message == WM_SYSKEYDOWN, GetForegroundWindow() == target)) return 1;
  }
  return CallNextHookEx(nullptr, code, message, data);
}

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
  puts("PASS keyboard native mappings, host suppression, Win+R, IME UP, foreground loss, paired cleanup and scoped synthetic modifiers; no hook installed");
  return 0;
}

int main(int argc, char **argv)
{
  if (argc == 2 && std::string(argv[1]) == "--self-test") return SelfTest();
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
  if (!hook) { CloseHandle(parent); return 2; }
  std::thread([] {
    std::string command;
    while (std::getline(std::cin, command)) {
      if (command.size() > 32) break;
      if ((command[0] == 'E' || command[0] == 'D') && command.size() > 2) {
        const unsigned value = static_cast<unsigned>(std::strtoul(command.c_str() + 2, nullptr, 10));
        PostThreadMessageW(messageThread, WM_APP + 1, command[0] == 'E', value);
      }
    }
    PostThreadMessageW(messageThread, WM_QUIT, 0, 0);
  }).detach();
  std::thread([] {
    for (;;) {
      const unsigned r = consumed.load(std::memory_order_relaxed);
      if (r == written.load(std::memory_order_acquire)) { Sleep(1); continue; }
      const Event e = events[r % events.size()];
      if (e.kind == 'K') std::printf("K %u %u %u\n", e.generation, e.usage, e.down);
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
      enabled = message.wParam != 0 && GetForegroundWindow() == target;
      Emit('S', enabled ? 1U : 0U);
    } else if (message.message == WM_TIMER) {
      if (!IsWindow(target) || WaitForSingleObject(parent, 0) == WAIT_OBJECT_0) break;
      if (enabled && GetForegroundWindow() != target) Reset();
    }
  }
  UnhookWindowsHookEx(hook); CloseHandle(parent);
  return failed ? 3 : 0;
}
