import { defineConfig } from '#q-app'
import path from 'node:path'

export default defineConfig(() => ({
  boot: [],
  css: ['app.scss'],
  extras: ['material-icons'],
  build: {
    target: {
      browser: ['es2022', 'chrome140'],
      node: 'node22'
    },
    typescript: {
      strict: true,
      vueShim: true
    },
    vueRouterMode: 'hash'
  },
  devServer: {
    open: false
  },
  framework: {
    config: {
      dark: true,
      notify: {
        position: 'bottom-right',
        timeout: 2800
      }
    },
    plugins: ['Notify', 'Dialog']
  },
  animations: [],
  electron: {
    preloadScripts: ['electron-preload'],
    inspectPort: 5858,
    bundler: 'packager',
    packager: {
      extraResource: [path.resolve('src-electron/native-bin/minikmb-capture.exe'), path.resolve('src-electron/native-bin/minikmb-keyboard.exe')],
      appBundleId: 'local.kbm.control',
      executableName: 'kbm-control'
    }
  }
}))

