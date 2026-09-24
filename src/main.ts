import './styles.css';
import { api } from './api';
import { Camera, CameraStatus, PrepareSetIpResult } from './shared/types';
import { ECMDP_CLIENT_VERSIONS, NetConfig, NetState, validateConfig, stripPrefix } from './shared/ecmdp';
import { statusMessage } from './shared/messages';

const FACTORY_PASSWORD = '3dvl';

const cameras = new Map<string, Camera>();

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

const cameraList = $('camera-list');
const emptyState = $('empty-state');
const statusEl = $('status');
const refreshBtn = $('refresh-btn');
const toastEl = $('toast');

const ICON_IDENTIFY = `
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
    <path d="M9 18h6M10 22h4M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2z"></path>
  </svg>`;

// --- Camera list ---

function statusLine(camera: Camera): { text: string; cls: string } {
  const info = camera.ecmdp;
  const address = info?.net.address ?? camera.ip;
  switch (camera.status) {
    case 'online':
      return { text: address, cls: '' };
    case 'other-subnet':
      return {
        text: `${address || 'no address'} · different subnet (this PC: ${info?.localCidr ?? '?'})`,
        cls: 'warn',
      };
    case 'unreachable':
      return { text: `${address} · web interface not responding`, cls: 'warn' };
    default:
      return { text: `${address || camera.ip} · offline`, cls: '' };
  }
}

function detailLine(camera: Camera): string {
  const info = camera.ecmdp;
  if (!info) return '';
  const parts = [
    `S/N ${info.serial}`, info.model, info.fwVersion && `FW ${info.fwVersion}`,
    `protocol ${info.versions.map(v => `v${v}`).join('/')}`,
  ].filter(Boolean);
  return parts.join(' · ');
}

function render(): void {
  cameraList.innerHTML = '';
  emptyState.classList.toggle('hidden', cameras.size > 0);

  const sorted = Array.from(cameras.values()).sort((a, b) => a.hostname.localeCompare(b.hostname));

  for (const camera of sorted) {
    const info = camera.ecmdp;
    const line = statusLine(camera);
    const detail = detailLine(camera);
    const incompatible = !!info && info.version === null;
    const versionHint = incompatible
      ? statusMessage('unsupported_version', undefined, { camera: info!.versions, app: ECMDP_CLIENT_VERSIONS })
      : '';
    const canSetIp = !!info && info.caps.includes('set_ip');
    const canIdentify = !!info && info.caps.includes('identify') && camera.status !== 'offline' && !incompatible;
    const setIpDisabledReason = incompatible
      ? versionHint
      : info?.setIp === 'disabled' ? 'Disabled in the camera\'s web interface' : '';
    const needsFix = camera.status === 'other-subnet' || camera.status === 'unreachable';

    const row = document.createElement('div');
    row.className = `camera-row status-${camera.status}`;
    row.dataset.id = camera.id;
    row.innerHTML = `
      <div class="status-dot"></div>
      <div class="camera-info">
        <div class="camera-hostname">
          <span>${escapeHtml(camera.hostname)}</span>
          ${info?.keyTrust === 'mismatch'
            ? '<span class="badge danger" title="The key of this camera differs from the one you trusted before.">Key changed</span>'
            : ''}
          ${incompatible ? `<span class="badge warn" title="${escapeHtml(versionHint)}">Incompatible</span>` : ''}
        </div>
        <div class="camera-ip ${line.cls}">${escapeHtml(line.text)}</div>
        ${detail ? `<div class="camera-detail">${escapeHtml(detail)}</div>` : ''}
      </div>
      <div class="camera-actions">
        ${canIdentify ? `<button class="icon-btn identify-btn" title="Identify (flash LED)">${ICON_IDENTIFY}</button>` : ''}
        ${canSetIp
          ? `<button class="btn setip-btn${needsFix && !setIpDisabledReason ? ' primary' : ''}"${setIpDisabledReason
            ? ` disabled title="${escapeHtml(setIpDisabledReason)}"`
            : ''}>Change IP</button>`
          : ''}
        <button class="btn open-btn"${camera.online ? '' : ' disabled'}>Open</button>
      </div>
    `;

    row.querySelector('.open-btn')!.addEventListener('click', () => api.openCamera(camera.id));
    row.querySelector('.setip-btn')?.addEventListener('click', () => openSetIpDialog(camera.id));
    row.querySelector('.identify-btn')?.addEventListener('click', (e) => identify(camera.id, e.currentTarget as HTMLButtonElement));

    cameraList.appendChild(row);
  }

  updateStatus();
}

function updateStatus(): void {
  const counts: Record<CameraStatus, number> = { 'online': 0, 'other-subnet': 0, 'unreachable': 0, 'offline': 0 };
  for (const camera of cameras.values()) counts[camera.status]++;

  if (cameras.size === 0) {
    statusEl.textContent = '0 cameras found';
    return;
  }
  const parts = [`${counts.online} online`];
  if (counts['other-subnet']) parts.push(`${counts['other-subnet']} in other subnet`);
  if (counts.unreachable) parts.push(`${counts.unreachable} not responding`);
  if (counts.offline) parts.push(`${counts.offline} offline`);
  statusEl.textContent = parts.join(', ');
}

function escapeHtml(str: string): string {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;
function toast(message: string, kind: 'info' | 'error' = 'info'): void {
  toastEl.textContent = message;
  toastEl.className = kind;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.add('hidden'), 4000);
}

async function identify(id: string, button: HTMLButtonElement): Promise<void> {
  button.disabled = true;
  button.classList.add('pulsing');
  try {
    const res = await api.identify(id);
    toast(res.message ?? (res.ok ? 'Identifying.' : 'Identify failed.'), res.ok ? 'info' : 'error');
  } finally {
    setTimeout(() => {
      button.disabled = false;
      button.classList.remove('pulsing');
    }, 2000);
  }
}

function handleRefresh(): void {
  refreshBtn.classList.add('spinning');
  // Remove offline cameras on refresh
  for (const [id, camera] of cameras) {
    if (!camera.online && camera.status === 'offline') cameras.delete(id);
  }
  render();
  api.refresh();
  setTimeout(() => refreshBtn.classList.remove('spinning'), 2000);
}

refreshBtn.addEventListener('click', handleRefresh);

const upsert = (camera: Camera) => {
  cameras.set(camera.id, camera);
  render();
  syncDialogCamera(camera);
};
// Listen first, then fetch the list, so no camera found in between is missed
Promise.all([
  api.onCameraFound(upsert),
  api.onCameraUpdated(upsert),
  api.onCameraRemoved((id) => {
    cameras.delete(id);
    render();
  }),
]).then(() => api.getCameras()).then((list) => {
  for (const camera of list) cameras.set(camera.id, camera);
  render();
});

// --- "Change IP" dialog (discovery-protocol.md §8.4, §8.5) ---

const dialog = $<HTMLDialogElement>('setip-dialog');
const form = $<HTMLFormElement>('setip-form');
const dlgCamera = $('dlg-camera');
const dlgCurrent = $('dlg-current');
const dlgLoading = $('dlg-loading');
const dlgBody = $('dlg-body');
const trustBox = $('trust-box');
const dlgMessage = $('dlg-message');
const applyBtn = $<HTMLButtonElement>('dlg-apply');
const cancelBtn = $<HTMLButtonElement>('dlg-cancel');
const staticFields = $('static-fields');
const dhcpFields = $('dhcp-fields');
const fAddress = $<HTMLInputElement>('f-address');
const fGateway = $<HTMLInputElement>('f-gateway');
const fFbAddress = $<HTMLInputElement>('f-fb-address');
const fFbGateway = $<HTMLInputElement>('f-fb-gateway');
const fDns = $<HTMLInputElement>('f-dns');
const fUser = $<HTMLInputElement>('f-user');
const fPassword = $<HTMLInputElement>('f-password');
const suggestionBox = $('suggestion');
const suggestionText = $('suggestion-text');
const suggestionBtn = $<HTMLButtonElement>('suggestion-apply');

interface DialogState {
  cameraId: string;
  fingerprint: string;
  // Key needs confirmation before anything is sent
  confirmNeeded: boolean;
  // The device consumes the challenge on every set_ip; fetch a new one before retrying
  challengeUsed: boolean;
  busy: boolean;
  done: boolean;
  touched: boolean;
}

let state: DialogState | null = null;

function modeValue(): 'static' | 'dhcp' {
  return (form.querySelector('input[name="mode"]:checked') as HTMLInputElement).value as 'static' | 'dhcp';
}

function setMode(mode: 'static' | 'dhcp'): void {
  (form.querySelector(`input[name="mode"][value="${mode}"]`) as HTMLInputElement).checked = true;
  staticFields.classList.toggle('hidden', mode !== 'static');
  dhcpFields.classList.toggle('hidden', mode !== 'dhcp');
}

function describeNet(net: NetState): string {
  if (net.mode === 'static') {
    return `Static ${net.address ?? '–'}${net.gateway ? `, gateway ${net.gateway}` : ''}`;
  }
  const fb = net.fallback ? ` (fallback ${net.fallback.address})` : '';
  return `DHCP${fb} · currently ${net.address ?? 'no address'}`;
}

// Validation messages come and go with the form content; result messages stay
// until the next attempt.
let showingValidation = false;

function showMessage(text: string, kind: 'error' | 'success' | 'info'): void {
  dlgMessage.textContent = text;
  dlgMessage.className = `dlg-message ${kind}`;
  showingValidation = false;
}

function hideMessage(): void {
  dlgMessage.className = 'dlg-message hidden';
  showingValidation = false;
}

function readConfig(): NetConfig {
  const dns = fDns.value.split(',').map(s => s.trim()).filter(Boolean);
  if (modeValue() === 'static') {
    return { mode: 'static', address: fAddress.value.trim(), gateway: fGateway.value.trim() || null, dns };
  }
  const fbAddress = fFbAddress.value.trim();
  return {
    mode: 'dhcp',
    fallback: fbAddress ? { address: fbAddress, gateway: fFbGateway.value.trim() || null } : null,
    dns,
  };
}

/** The camera's current settings, as a starting point for the form. */
function currentConfig(net: NetState): NetConfig {
  return { mode: net.mode, address: net.address ?? '', gateway: net.gateway, dns: net.dns, fallback: net.fallback };
}

function fillConfig(config: NetConfig): void {
  setMode(config.mode);
  // Static fields also hold the current address in DHCP mode, so switching
  // to "Static" starts from what the camera has now.
  fAddress.value = config.address ?? '';
  fGateway.value = config.gateway ?? '';
  fFbAddress.value = config.fallback?.address ?? '';
  fFbGateway.value = config.fallback?.gateway ?? '';
  fDns.value = (config.dns ?? []).join(', ');
}

function describeConfig(config: NetConfig): string {
  return config.mode === 'static' ? `static ${config.address}` : 'DHCP';
}

/** Offer the suggested config when the camera cannot be reached from this PC. */
function renderSuggestion(camera: Camera, suggestion: NetConfig | undefined): void {
  const info = camera.ecmdp!;
  const needed = camera.status === 'other-subnet' || camera.status === 'unreachable';
  if (!needed || !suggestion || (suggestion.mode === 'static' && !suggestion.address)) {
    suggestionBox.classList.add('hidden');
    return;
  }
  suggestionText.textContent =
    `Not reachable from this PC (${info.localCidr}). Suggested: ${describeConfig(suggestion)}`;
  suggestionBox.classList.remove('hidden');
  suggestionBtn.onclick = () => {
    const current = currentConfig(info.net);
    fillConfig({
      ...suggestion,
      // Keep the camera's fallback unless the suggestion sets one
      fallback: suggestion.fallback ?? current.fallback,
      dns: current.dns,
    });
    if (state) state.touched = true;
    updateApply();
  };
}

/** Returns an error message for the network settings, or null. */
function configError(): string | null {
  if (!state) return null;
  const config = readConfig();
  const err = validateConfig(config);
  if (err) return err;
  const camera = cameras.get(state.cameraId);
  const local = camera?.ecmdp?.localAddress;
  if (config.mode === 'static' && local && stripPrefix(config.address ?? null) === local) {
    return 'This is the address of your PC. Choose a different one.';
  }
  return null;
}

function credentialsMissing(): boolean {
  return !fUser.value.trim() || !fPassword.value;
}

function trustConfirmed(): boolean {
  const box = trustBox.querySelector<HTMLInputElement>('input[type="checkbox"]');
  return !state?.confirmNeeded || !!box?.checked;
}

function updateApply(): void {
  if (!state) return;
  const err = configError();
  applyBtn.disabled = state.busy || !!err || credentialsMissing() || !trustConfirmed();
  if (state.touched && err) {
    showMessage(err, 'error');
    showingValidation = true;
  } else if (showingValidation) {
    hideMessage();
  }
}

function renderTrust(res: PrepareSetIpResult): void {
  const fp = escapeHtml(res.fingerprint ?? '');
  if (res.trust === 'match') {
    trustBox.className = 'trust-box ok';
    trustBox.innerHTML = `<span>Camera key verified</span><code>${fp}</code>`;
    return;
  }
  if (res.trust === 'new') {
    trustBox.className = 'trust-box info';
    trustBox.innerHTML = `
      <strong>First connection to this camera</strong>
      <p>Key fingerprint:</p>
      <code>${fp}</code>
      <p>You can compare it with the fingerprint shown in the camera's web interface.</p>
      <label class="check"><input type="checkbox"> I trust this camera</label>`;
  } else {
    trustBox.className = 'trust-box danger';
    trustBox.innerHTML = `
      <strong>The key of this camera has changed</strong>
      <p>This is expected after a factory reset. Otherwise, another device on the network may be
        impersonating the camera. Do not enter your password if you are unsure.</p>
      <p>Previously trusted:</p>
      <code>${escapeHtml(res.pinnedFingerprint ?? '')}</code>
      <p>Now:</p>
      <code>${fp}</code>
      <label class="check"><input type="checkbox"> Trust the new key</label>`;
  }
  trustBox.querySelector('input')!.addEventListener('change', updateApply);
}

function setBusy(busy: boolean): void {
  if (!state) return;
  state.busy = busy;
  for (const el of form.querySelectorAll<HTMLInputElement>('input')) el.disabled = busy;
  applyBtn.textContent = busy ? 'Applying...' : 'Apply';
  updateApply();
}

function syncDialogCamera(camera: Camera): void {
  if (!state || state.cameraId !== camera.id || !camera.ecmdp) return;
  dlgCurrent.textContent = `Current: ${describeNet(camera.ecmdp.net)}`;
}

async function openSetIpDialog(id: string): Promise<void> {
  const camera = cameras.get(id);
  if (!camera?.ecmdp) return;

  state = {
    cameraId: id, fingerprint: '', confirmNeeded: false, challengeUsed: false,
    busy: false, done: false, touched: false,
  };
  dlgCamera.textContent = `${camera.hostname} · S/N ${camera.ecmdp.serial} · ${camera.ecmdp.mac}`;
  dlgCurrent.textContent = `Current: ${describeNet(camera.ecmdp.net)}`;
  dlgLoading.classList.remove('hidden');
  dlgBody.classList.add('hidden');
  hideMessage();
  fPassword.value = '';
  applyBtn.classList.remove('hidden');
  applyBtn.disabled = true;
  cancelBtn.textContent = 'Cancel';
  dialog.showModal();

  const res = await api.prepareSetIp(id);
  if (!state || state.cameraId !== id) return; // dialog closed meanwhile
  dlgLoading.classList.add('hidden');

  if (!res.ok) {
    showMessage(res.error ?? 'The camera could not be contacted.', 'error');
    applyBtn.classList.add('hidden');
    cancelBtn.textContent = 'Close';
    return;
  }

  state.fingerprint = res.fingerprint!;
  state.confirmNeeded = res.trust !== 'match';
  renderTrust(res);
  fillConfig(currentConfig(camera.ecmdp.net));
  renderSuggestion(camera, res.suggestion);
  dlgBody.classList.remove('hidden');
  updateApply();
  fPassword.focus({ preventScroll: true });
  dialog.scrollTop = 0;
}

/** Get a fresh challenge before a retry. Returns false if the key changed. */
async function renewChallenge(): Promise<boolean> {
  const res = await api.prepareSetIp(state!.cameraId);
  if (!res.ok) {
    showMessage(res.error ?? 'The camera could not be contacted.', 'error');
    return false;
  }
  if (res.fingerprint !== state!.fingerprint) {
    state!.fingerprint = res.fingerprint!;
    state!.confirmNeeded = res.trust !== 'match';
    renderTrust(res);
    showMessage('The camera key changed. Please check it before continuing.', 'error');
    return false;
  }
  state!.challengeUsed = false;
  return true;
}

async function submit(): Promise<void> {
  if (!state || state.busy || state.done) return;
  state.touched = true;
  if (configError() || credentialsMissing() || !trustConfirmed()) {
    updateApply();
    return;
  }

  setBusy(true);
  hideMessage();
  try {
    if (state.challengeUsed && !(await renewChallenge())) return;

    const config = readConfig();
    const usedFactoryPassword = fPassword.value === FACTORY_PASSWORD;
    const res = await api.setIp({
      id: state.cameraId,
      config,
      user: fUser.value.trim(),
      password: fPassword.value,
      acceptKey: state.confirmNeeded,
    });
    state.challengeUsed = true;
    // A key confirmed once is stored in the trust store
    if (res.status !== 'untrusted' && res.status !== 'key_changed') state.confirmNeeded = false;

    if (res.status === 'ok') {
      state.done = true;
      const address = res.net?.address ?? (config.mode === 'static' ? config.address : 'DHCP');
      let text = `IP configuration applied (${address}). Searching for the camera...`;
      if (usedFactoryPassword) text += '\nTip: change the factory password in the camera\'s web interface.';
      showMessage(text, 'success');
      dlgBody.classList.add('hidden');
      applyBtn.classList.add('hidden');
      cancelBtn.textContent = 'Close';
      fPassword.value = '';
      return;
    }

    showMessage(res.message ?? statusMessage(res.status), 'error');
    if (res.status === 'auth_failed') {
      fPassword.value = '';
      fPassword.focus();
    }
  } catch (err) {
    showMessage(`Unexpected error: ${(err as Error).message}`, 'error');
  } finally {
    setBusy(false);
  }
}

function closeDialog(): void {
  if (state?.busy) return;
  fPassword.value = '';
  state = null;
  dialog.close();
}

form.addEventListener('submit', (e) => {
  e.preventDefault();
  submit();
});
form.addEventListener('input', updateApply);
// Show validation errors once the user leaves a field
form.addEventListener('focusout', () => {
  if (!state || state.done) return;
  state.touched = true;
  updateApply();
});
for (const radio of form.querySelectorAll<HTMLInputElement>('input[name="mode"]')) {
  radio.addEventListener('change', () => {
    setMode(modeValue());
    updateApply();
  });
}
cancelBtn.addEventListener('click', closeDialog);
dialog.addEventListener('cancel', (e) => {
  e.preventDefault();
  closeDialog();
});
