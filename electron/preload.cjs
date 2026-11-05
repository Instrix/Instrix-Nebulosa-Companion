// What the page gets from the Electron shell (ui/src/lib/ipc.ts, `Shell`):
// file dialogs, the zoom (UI scale) and the paths of dropped files.
'use strict';
const { contextBridge, ipcRenderer, webUtils } = require('electron');

contextBridge.exposeInMainWorld('nebulosaShell', {
  openFile: (filters) => ipcRenderer.invoke('nebulosa:open-file', filters),
  saveFile: (defaultPath, filters) => ipcRenderer.invoke('nebulosa:save-file', defaultPath, filters),
  setZoom: (f) => ipcRenderer.send('nebulosa:zoom', f),
  onDropPaths: (cb) => {
    const over = (e) => e.preventDefault();
    const drop = (e) => {
      e.preventDefault();
      const paths = [...(e.dataTransfer?.files ?? [])].map((f) => webUtils.getPathForFile(f)).filter(Boolean);
      if (paths.length) cb(paths);
    };
    window.addEventListener('dragover', over);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('dragover', over);
      window.removeEventListener('drop', drop);
    };
  },
});
