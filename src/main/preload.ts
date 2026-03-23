import { contextBridge, ipcRenderer } from 'electron';
import { IPC, Camera } from '../shared/types';

contextBridge.exposeInMainWorld('electronAPI', {
  onCameraFound: (callback: (camera: Camera) => void) => {
    ipcRenderer.on(IPC.CAMERA_FOUND, (_event, camera) => callback(camera));
  },
  onCameraLost: (callback: (camera: Camera) => void) => {
    ipcRenderer.on(IPC.CAMERA_LOST, (_event, camera) => callback(camera));
  },
  getCameras: (): Promise<Camera[]> => {
    return ipcRenderer.invoke(IPC.CAMERA_LIST);
  },
  refresh: () => {
    ipcRenderer.send(IPC.REFRESH);
  },
  openCamera: (id: string) => {
    ipcRenderer.send(IPC.OPEN, id);
  },
});
