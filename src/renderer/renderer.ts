import './styles.css';
import { Camera } from '../shared/types';

interface ElectronAPI {
  onCameraFound: (callback: (camera: Camera) => void) => void;
  onCameraLost: (callback: (camera: Camera) => void) => void;
  getCameras: () => Promise<Camera[]>;
  refresh: () => void;
  openCamera: (id: string) => void;
}

declare global {
  interface Window {
    electronAPI: ElectronAPI;
  }
}

const cameras = new Map<string, Camera>();

const cameraList = document.getElementById('camera-list')!;
const emptyState = document.getElementById('empty-state')!;
const statusEl = document.getElementById('status')!;
const refreshBtn = document.getElementById('refresh-btn')!;

function render(): void {
  cameraList.innerHTML = '';

  if (cameras.size === 0) {
    emptyState.classList.remove('hidden');
  } else {
    emptyState.classList.add('hidden');
  }

  const sorted = Array.from(cameras.values()).sort((a, b) =>
    a.hostname.localeCompare(b.hostname)
  );

  for (const camera of sorted) {
    const row = document.createElement('div');
    row.className = camera.online ? 'camera-row' : 'camera-row offline';
    row.dataset.id = camera.id;

    row.innerHTML = `
      <div class="status-dot"></div>
      <div class="camera-info">
        <div class="camera-hostname">${escapeHtml(camera.hostname)}</div>
        <div class="camera-ip">${escapeHtml(camera.ip)}</div>
      </div>
      <button class="open-btn"${camera.online ? '' : ' disabled'}>Open</button>
    `;

    if (camera.online) {
      const openBtn = row.querySelector('.open-btn')!;
      openBtn.addEventListener('click', () => {
        window.electronAPI.openCamera(camera.id);
      });
    }

    cameraList.appendChild(row);
  }

  updateStatus();
}

function updateStatus(): void {
  const total = cameras.size;
  const online = Array.from(cameras.values()).filter(c => c.online).length;
  const offline = total - online;
  if (total === 0) {
    statusEl.textContent = '0 cameras found';
  } else if (offline === 0) {
    statusEl.textContent = `${online} online`;
  } else {
    statusEl.textContent = `${online} online, ${offline} offline`;
  }
}

function escapeHtml(str: string): string {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

function handleRefresh(): void {
  refreshBtn.classList.add('spinning');
  // Remove offline cameras on refresh
  for (const [id, camera] of cameras) {
    if (!camera.online) cameras.delete(id);
  }
  render();
  window.electronAPI.refresh();
  setTimeout(() => refreshBtn.classList.remove('spinning'), 2000);
}

refreshBtn.addEventListener('click', handleRefresh);

window.electronAPI.onCameraFound((camera) => {
  cameras.set(camera.id, camera);
  render();
});

window.electronAPI.onCameraLost((camera) => {
  const existing = cameras.get(camera.id);
  if (existing) {
    existing.online = false;
    render();
  }
});

window.electronAPI.getCameras().then((list) => {
  for (const camera of list) {
    cameras.set(camera.id, camera);
  }
  render();
});
