import { app, BrowserWindow, ipcMain, shell, nativeImage, Menu, NativeImage } from 'electron';
import * as path from 'path';
import { Scanner } from './scanner';
import { IPC, Camera } from '../shared/types';

declare const MAIN_WINDOW_WEBPACK_ENTRY: string;
declare const MAIN_WINDOW_PRELOAD_WEBPACK_ENTRY: string;

if (require('electron-squirrel-startup')) {
  app.quit();
}

// Set app name early so macOS menu bar and dock show correct name
app.setName('Edge Camera Discovery');

const DEMO_MODE = process.argv.includes('--demo');

let mainWindow: BrowserWindow | null = null;
const scanner = new Scanner();

const DEMO_CAMERAS: Camera[] = [
  { id: 'demo-1', hostname: 'ecm-00A1B2C3', ip: '192.168.1.101', port: 443, lastSeen: Date.now(), online: true },
  { id: 'demo-2', hostname: 'ecm-00D4E5F6', ip: '192.168.1.102', port: 443, lastSeen: Date.now(), online: true },
  { id: 'demo-3', hostname: 'ecm-00789ABC', ip: '192.168.1.103', port: 443, lastSeen: Date.now(), online: true },
  { id: 'demo-4', hostname: 'ecm-00DEF012', ip: '192.168.1.104', port: 443, lastSeen: Date.now() - 60000, online: false },
];

function createWindow(): void {
  // Resolve window icon (Linux/Windows — macOS uses the dock icon instead)
  const iconCandidates = [
    path.join(process.resourcesPath, 'icon.png'),                   // packaged (extraResource)
    path.join(__dirname, '..', '..', 'src', 'icons', 'icon.png'),   // dev mode
  ];
  let windowIcon: NativeImage | undefined;
  for (const candidate of iconCandidates) {
    const img = nativeImage.createFromPath(candidate);
    if (!img.isEmpty()) {
      windowIcon = img;
      break;
    }
  }

  mainWindow = new BrowserWindow({
    width: 720,
    height: 500,
    minWidth: 480,
    minHeight: 360,
    ...(windowIcon ? { icon: windowIcon } : {}),
    webPreferences: {
      preload: MAIN_WINDOW_PRELOAD_WEBPACK_ENTRY,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  mainWindow.loadURL(MAIN_WINDOW_WEBPACK_ENTRY);

  // Push known cameras after the renderer JS has had time to initialize.
  // did-finish-load fires when HTML loads, but the webpack bundle needs
  // additional time to execute and register IPC listeners.
  mainWindow.webContents.on('did-finish-load', () => {
    setTimeout(() => {
      const cameras = DEMO_MODE ? DEMO_CAMERAS : scanner.getAll();
      for (const camera of cameras) {
        mainWindow?.webContents.send(IPC.CAMERA_FOUND, camera);
      }
    }, 500);
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

function setupIPC(): void {
  scanner.on('camera-found', (camera: Camera) => {
    mainWindow?.webContents.send(IPC.CAMERA_FOUND, camera);
  });

  scanner.on('camera-lost', (camera: Camera) => {
    mainWindow?.webContents.send(IPC.CAMERA_LOST, camera);
  });

  ipcMain.handle(IPC.CAMERA_LIST, () => {
    return DEMO_MODE ? DEMO_CAMERAS : scanner.getAll();
  });

  ipcMain.on(IPC.REFRESH, () => {
    if (!DEMO_MODE) scanner.refresh();
  });

  ipcMain.on(IPC.OPEN, (_event, id: string) => {
    const camera = scanner.getById(id);
    if (camera) {
      shell.openExternal(`https://${camera.ip}`);
    }
  });
}

app.on('ready', () => {
  // macOS requires a menu bar — set a minimal one (About, Hide, Quit).
  // Windows/Linux: remove the menu bar entirely.
  if (process.platform === 'darwin') {
    const template: Electron.MenuItemConstructorOptions[] = [{
      label: 'Edge Camera Discovery',
      submenu: [
        { role: 'about' },
        { type: 'separator' },
        { role: 'hide' },
        { role: 'hideOthers' },
        { role: 'unhide' },
        { type: 'separator' },
        { role: 'quit' },
      ],
    }];
    Menu.setApplicationMenu(Menu.buildFromTemplate(template));
  } else {
    Menu.setApplicationMenu(null);
  }

  // Set dock icon on macOS (especially needed in dev mode)
  if (process.platform === 'darwin' && app.dock) {
    const candidates = [
      path.join(app.getAppPath(), 'src', 'icons', 'icon.png'),
      path.join(__dirname, '..', '..', 'src', 'icons', 'icon.png'),
    ];
    for (const iconPath of candidates) {
      const icon = nativeImage.createFromPath(iconPath);
      if (!icon.isEmpty()) {
        app.dock.setIcon(icon);
        break;
      }
    }
  }

  createWindow();
  setupIPC();
  if (!DEMO_MODE) scanner.start();
  if (DEMO_MODE) console.log('[DEMO] Running with fake cameras — no network scanning');

});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    createWindow();
  }
});

app.on('before-quit', () => {
  scanner.stop();
});
