# Keyboard capture

With special-key forwarding OFF, focused video uses normal application keyboard events. With it ON, the native Windows helper captures physical input for the active channel and suppresses the corresponding host handling within the focused capture scope. Mouse and keyboard have independent queues and completion tickets.

The helper is an in-process DLL with its own message thread. Callback processing does not perform serial I/O or file logging. Win32 reentrancy, host GUI messages and duplicate key paths are handled separately. Key-up is routed to the original channel/session that accepted key-down; pending input is cancelled on focus/selection/source changes and neutral reports are requested.

Han/Yeong is a physical HID language-key action rather than Unicode text injection. The host/target layout and IME still determine the resulting text. Repeated key actions need distinct completed down/up transitions. Key arrays use the same physical mappings and preserve order without requiring another REST/MCP call between transitions.

A HID_DONE ticket confirms endpoint completion, not the target OS/application's handling of the key. Do not retry an uncertain key-down automatically. Aggregate callback/suppression diagnostics do not record key contents. See ../native-keyboard/ and INPUT-SEQUENCES.md.

Win32 reference: https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc
