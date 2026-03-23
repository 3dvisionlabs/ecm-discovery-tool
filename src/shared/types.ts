export interface Camera {
  id: string;
  hostname: string;
  ip: string;
  port: number;
  lastSeen: number;
  online: boolean;
}

export const IPC = {
  CAMERA_FOUND: 'cameras:found',
  CAMERA_LOST: 'cameras:lost',
  CAMERA_LIST: 'cameras:list',
  REFRESH: 'cameras:refresh',
  OPEN: 'cameras:open',
} as const;
