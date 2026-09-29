// Backend API: Tauri commands (app/src/lib.rs) and camera events.
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Camera, IdentifyResult, PrepareSetIpResult, SetIpRequest, SetIpResult } from './shared/types';

const on = <T>(event: string, callback: (payload: T) => void): Promise<UnlistenFn> =>
  listen<T>(event, e => callback(e.payload));

export const api = {
  onCameraFound: (callback: (camera: Camera) => void) => on('cameras:found', callback),
  onCameraUpdated: (callback: (camera: Camera) => void) => on('cameras:updated', callback),
  // Legacy mDNS entry merged into an FDP entry
  onCameraRemoved: (callback: (id: string) => void) => on('cameras:removed', callback),
  getCameras: () => invoke<Camera[]>('get_cameras'),
  refresh: () => {
    void invoke('refresh');
  },
  openCamera: (id: string) => {
    void invoke('open_camera', { id });
  },
  prepareSetIp: (id: string) => invoke<PrepareSetIpResult>('prepare_set_ip', { id }),
  // Store the key from the last prepareSetIp as trusted (user confirmed it)
  trustCamera: (id: string) => invoke<boolean>('trust_camera', { id }),
  setIp: (req: SetIpRequest) => invoke<SetIpResult>('set_ip', { req }),
  identify: (id: string) => invoke<IdentifyResult>('identify', { id }),
};
