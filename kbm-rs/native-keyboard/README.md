# In-process native keyboard DLL

The Rust GUI loads tools/minikmb-keyboard.dll. The GUI window owner installs
WH_KEYBOARD_LL in the same PID; a dedicated C++ thread maintains the lease,
commands and cleanup. Rust polls a static 512-entry memory queue
through a plain C ABI: no runtime child process, pipe or Rust hook callback.

Thread-specific WH_KEYBOARD and WH_GETMESSAGE
hooks intercept the GUI input queue before winit's observer. Posted messages
have their own fallback, with physical/message-path deduplication. A short SRW
guard serializes only native policy/queue updates; callbacks never call Rust,
log, wait for MCU replies or perform disk/pipe/MCU I/O. LANG1/2 are atomic DOWN/UP pulses;
the GUI previous-state bit rejects auto-repeat without leaving a target key held.
The original 40-byte stats export remains ABI-compatible; new clients use
kbm_status_v2 with GUI callback/block counters. Its threadId identifies the
GUI thread that installed WH_KEYBOARD_LL, not the maintenance worker.

When Windows has already accepted a captured Win DOWN before the window hook,
the scoped fallback sends an unassigned E8 mask pair and a tagged local Win UP.
The local correction bypasses target routing; physical Raw Input UP ends the
original target Win hold. The mask is sent outside the policy lock, only with
the active foreground window/epoch/lease. It is not used for preheld host keys.
Self-tests inspect the INPUT plan but never call SendInput. Optional
MINIKMB_KEYBOARD_TRACE=1 enables transient Win counters in a per-PID mapping;
normal execution does not enable that diagnostic flag or retain typed contents.

Build with installed MSVC x64 tools and Python:

    python scripts/build-native-keyboard.py

The script stages the DLL next to release/debug binaries. The development EXE
is for no-hook mapping/self-tests only and is not included in release ZIPs.
The DLL, hash manifest, C++ source and build script are included. No downloads.

Only the exact foreground HWND and a live 750ms GUI lease allow captured DOWNs.
A new DOWN passed to the host retains its host UP, even if an old suppressed
marker exists. Scope loss releases the original target owner. Callbacks perform
no logging, pipe I/O or calls into Rust. Code is unloaded only after the native
thread stops; unconfirmed shutdown disables capture and retains the module.

Test mode installs no hook. Synthetic memory-queue events go to a mock MCU;
tests never inject real OS input or send real device commands. An unarmed hidden
window test posts only its own queue messages to verify the GUI message hook.
The development EXE's
--self-test/--map-test modes also install no hook.
