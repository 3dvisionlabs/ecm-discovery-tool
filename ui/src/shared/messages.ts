// User-facing texts for FDP results, shared by main (prepare/identify) and
// renderer (set_ip). The device's own `message` is shown as a detail line.
import type { SetIpResult } from './types';

type Status = SetIpResult['status'];

const STATUS_TEXT: Record<Status, string> = {
  ok: 'Done.',
  auth_failed: 'Wrong user name or password.',
  bad_request:
    'The camera could not read the request. The app and the camera firmware probably implement '
    + 'different versions of the discovery protocol. Update the app or the camera firmware.',
  invalid_config: 'The camera rejected the network settings.',
  disabled: 'Changing the IP address via the discovery tool is disabled in the camera\'s web interface.',
  locked: 'Too many failed attempts. Please wait 5 minutes and try again.',
  error: 'The camera reported an internal error and did not change its settings.',
  timeout: 'The camera did not respond. Check the connection and try again.',
  untrusted: 'The camera key was not confirmed. Nothing was sent.',
  key_changed: 'The camera key changed while the dialog was open. Nothing was sent. Please check the key and try again.',
  unsupported_version: 'The camera firmware and this app speak different versions of the discovery protocol.',
};

export interface VersionInfo {
  camera: number[];
  app: number[];
}

function versionAdvice(v: VersionInfo): string {
  const list = (xs: number[]) => xs.map(x => `v${x}`).join(', ');
  const update = Math.max(...v.camera) < Math.max(...v.app) ? 'Update the camera firmware.' : 'Update this app.';
  return `Camera: ${list(v.camera)}, app: ${list(v.app)}. ${update}`;
}

/** Explanation for a status, plus the camera's own message if it adds anything. */
export function statusMessage(status: string, detail?: string, versions?: VersionInfo): string {
  let text = STATUS_TEXT[status as Status] ?? `The camera answered with an unknown status "${status}".`;
  if (versions) text += ` ${versionAdvice(versions)}`;
  return detail && detail !== text ? `${text}\nCamera message: ${detail}` : text;
}
