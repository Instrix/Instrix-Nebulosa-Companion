// The Linux desktop app: a Chromium window (Electron) on the Rust backend.
// It starts nebulosa-backend (the same backend as the Tauri app, behind a
// WebSocket on 127.0.0.1, src-tauri/src/server.rs), opens the URL it prints,
// and stops it on quit. The window gets file dialogs, zoom and dropped files
// through preload.cjs; nothing else of Node reaches the page.
'use strict';
const { app, BrowserWindow, Menu, dialog, ipcMain, shell } = require('electron');
const { spawn } = require('node:child_process');
const path = require('node:path');
const fs = require('node:fs');

const res = (...p) => (app.isPackaged ? path.join(process.resourcesPath, ...p) : path.join(__dirname, '..', ...p));
const BACKEND = app.isPackaged ? res('nebulosa-backend') : res('target', 'release', 'nebulosa-backend');
const DIST = app.isPackaged ? res('dist') : res('dist');
const ICON = app.isPackaged ? res('icon.png') : res('src-tauri', 'icons', 'icon.png');

let backend = null;
let win = null;
let origin = '';

function startBackend() {
  return new Promise((resolve, reject) => {
    if (!fs.existsSync(BACKEND)) return reject(new Error(`backend not found: ${BACKEND}`));
    backend = spawn(BACKEND, ['--dist', DIST, '--exit-with-stdin'], { stdio: ['pipe', 'pipe', 'inherit'] });
    let out = '';
    backend.stdout.on('data', (d) => {
      out += d;
      const m = /NEBULOSA_BACKEND (\S+)/.exec(out);
      if (m) resolve(m[1]);
    });
    backend.on('error', reject);
    backend.on('exit', (code) => {
      backend = null;
      if (!origin) reject(new Error(`backend exited (${code})`));
      else if (!app.isQuitting) {
        dialog.showErrorBox('Nebulosa Companion', `The backend stopped (exit ${code}). The app will close.`);
        app.quit();
      }
    });
  });
}

function createWindow(url) {
  win = new BrowserWindow({
    width: 1440,
    height: 900,
    minWidth: 960,
    minHeight: 640,
    title: 'Nebulosa Companion',
    icon: ICON,
    backgroundColor: '#11131a',
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      // the plots keep drawing when the window is behind another
      backgroundThrottling: false,
    },
  });
  win.once('ready-to-show', () => win.show());
  // only our pages in the window; links (help, docs) in the user's browser
  win.webContents.setWindowOpenHandler(({ url: u }) => {
    if (/^https?:/.test(u)) shell.openExternal(u);
    return { action: 'deny' };
  });
  win.webContents.on('will-navigate', (e, u) => {
    if (!u.startsWith(origin)) {
      e.preventDefault();
      if (/^https?:/.test(u)) shell.openExternal(u);
    }
  });
  // F12 / Ctrl+Shift+I: the developer tools (support)
  win.webContents.on('before-input-event', (e, i) => {
    if (i.type === 'keyDown' && (i.key === 'F12' || (i.control && i.shift && i.key.toLowerCase() === 'i'))) win.webContents.toggleDevTools();
  });
  win.loadURL(url);
}

ipcMain.handle('nebulosa:open-file', async (_e, filters) => {
  const r = await dialog.showOpenDialog(win, { properties: ['openFile'], filters });
  return r.canceled ? null : r.filePaths[0] ?? null;
});
ipcMain.handle('nebulosa:save-file', async (_e, defaultPath, filters) => {
  const r = await dialog.showSaveDialog(win, { defaultPath, filters });
  return r.canceled ? null : r.filePath ?? null;
});
ipcMain.on('nebulosa:zoom', (_e, f) => {
  if (win && typeof f === 'number' && f > 0.25 && f < 4) win.webContents.setZoomFactor(f);
});

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (win) {
      if (win.isMinimized()) win.restore();
      win.focus();
    }
  });
  // no menu bar: the app has its own keys (Ctrl K, Ctrl +/−…)
  Menu.setApplicationMenu(null);
  app.whenReady().then(async () => {
    try {
      const url = await startBackend();
      origin = new URL(url).origin;
      createWindow(url);
    } catch (e) {
      dialog.showErrorBox('Nebulosa Companion', `Could not start: ${e.message}`);
      app.quit();
    }
  });
  app.on('before-quit', () => {
    app.isQuitting = true;
    if (backend) backend.kill();
  });
  app.on('window-all-closed', () => app.quit());
}
