# MiniKMB Electron app (v1.0)

Electron + Quasar + Vue 3 + TypeScript app, retained alongside the native rewrite. The `src-electron/` main process owns serial transport, storage/network services and native keyboard hooks; the renderer is sandboxed. RP2040 protocol adapters and current MiniKMB support are both retained in source.

From this directory: `npm ci`, then `npm run typecheck` and `npm run build`. Install the main-process dependencies with `npm ci --prefix src-electron` when building from a fresh checkout. Windows/MSVC is needed for the native helper. `../work/build-minikmb-native.cjs` is the prebuild step.

Run regression tests from the repository root with `node work/<name>-tests.cjs`. They use mocked transports and do not send real HID. See `../docs/BUILDING.md` and `../docs/TESTING.md`.

MIT for MiniKMB-authored code; retain third-party package licenses. Exact package sources are in `../vendor/npm/`; lock files specify versions and integrity values.
