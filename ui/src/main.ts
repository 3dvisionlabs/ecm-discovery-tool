import './styles.css';
import { api } from './api';
import { Camera, CameraStatus, IdentifyResult, PrepareSetIpResult, SetIpResult } from './shared/types';
import { FDP_CLIENT_VERSIONS, NetConfig, NetState, validateConfig, stripPrefix } from './shared/fdp';
import { statusMessage } from './shared/messages';

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

/** State badge next to the name; empty text for reachable cameras. */
function stateLine(camera: Camera): { text: string; cls: string; title?: string } {
  switch (camera.status) {
    case 'online':
      return { text: '', cls: '' };
    case 'other-subnet':
      // What counts is reachability; the subnet only explains why it fails
      return {
        text: 'Not reachable',
        cls: 'warn',
        title: `The address is outside the network of this PC (${camera.fdp?.localCidr ?? '?'}) and there is no `
          + 'route to it. "Change IP" can move the camera into this network.',
      };
    case 'unreachable':
      return { text: 'Web UI not responding', cls: 'warn', title: 'The camera answers, but its web interface does not.' };
    default:
      return { text: 'Offline', cls: 'muted' };
  }
}

/** Label/value rows above the IP line; only FDP cameras report them. */
function detailRows(camera: Camera): [string, string][] {
  const info = camera.fdp;
  if (!info) return [];
  const rows: [string, string][] = [['Serial', info.serial]];
  if (info.model) rows.push(['Model', info.model]);
  if (info.fwVersion) rows.push(['Firmware', info.fwVersion]);
  return rows;
}

function render(): void {
  cameraList.innerHTML = '';
  emptyState.classList.toggle('hidden', cameras.size > 0);

  const sorted = Array.from(cameras.values()).sort((a, b) => a.hostname.localeCompare(b.hostname));

  for (const camera of sorted) {
    const info = camera.fdp;
    const address = camera.fdp?.net.address ?? camera.ip;
    const state = stateLine(camera);
    const details = detailRows(camera)
      .map(([label, value]) => `<dt>${label}</dt><dd>${escapeHtml(value)}</dd>`)
      .join('');
    const incompatible = !!info && info.version === null;
    const versionHint = incompatible
      ? statusMessage('unsupported_version', undefined, { camera: info!.versions, app: FDP_CLIENT_VERSIONS })
      : '';
    const canIdentify = !!info && info.caps.includes('identify') && camera.status !== 'offline' && !incompatible;
    const setIpUnavailable = setIpUnavailableReason(info, versionHint);
    const preparing = preparingId === camera.id;
    const flashing = identifying.has(camera.id);
    const needsFix = camera.status === 'other-subnet' || camera.status === 'unreachable';

    const row = document.createElement('div');
    row.className = `camera-row status-${camera.status}`;
    row.dataset.id = camera.id;
    row.innerHTML = `
      <div class="status-dot"></div>
      <div class="camera-info">
        <div class="camera-hostname">
          <span>${escapeHtml(camera.hostname)}</span>
          ${state.text
            ? `<span class="badge ${state.cls}"${state.title ? ` title="${escapeHtml(state.title)}"` : ''}>${escapeHtml(state.text)}</span>`
            : ''}
          ${info?.keyTrust === 'mismatch'
            ? '<span class="badge danger" title="The key of this camera differs from the one you trusted before.">Key changed</span>'
            : ''}
          ${incompatible ? `<span class="badge warn" title="${escapeHtml(versionHint)}">Incompatible</span>` : ''}
        </div>
        <dl class="camera-details">
          ${details}
          <dt>IP</dt><dd>${escapeHtml(address || 'no address')}</dd>
        </dl>
      </div>
      <div class="camera-actions">
        ${canIdentify
          ? `<button class="icon-btn identify-btn${flashing ? ' pulsing' : ''}"${flashing
            ? ' disabled title="The camera LED is flashing"'
            : ' title="Identify (flash LED)"'}>${ICON_IDENTIFY}</button>`
          : ''}
        <button class="btn setip-btn${setIpUnavailable ? ' unavailable' : needsFix ? ' primary' : ''}"${setIpUnavailable
          ? ` title="${escapeHtml(setIpUnavailable)}"`
          : ''}${preparing ? ' disabled' : ''}>${preparing ? 'Contacting...' : 'Change IP'}</button>
        <button class="btn open-btn"${camera.online ? '' : ' disabled'}>Open</button>
      </div>
    `;

    row.querySelector('.open-btn')!.addEventListener('click', () => api.openCamera(camera.id));
    row.querySelector('.setip-btn')!.addEventListener('click', () => {
      if (setIpUnavailable) toast(setIpUnavailable, 'info', 8000);
      else startChangeIp(camera.id);
    });
    row.querySelector('.identify-btn')?.addEventListener('click', () => identify(camera.id));

    cameraList.appendChild(row);
  }

  updateStatus();
}

/** Why "Change IP" is not possible for this camera, or '' if it is. */
function setIpUnavailableReason(info: Camera['fdp'], versionHint: string): string {
  const useWebUi = 'You can change the network settings in the camera\'s web interface instead.';
  if (!info) return `Changing the IP address from this app needs a newer camera firmware. ${useWebUi}`;
  if (!info.caps.includes('set_ip')) return `This camera does not support changing the IP address from this app. ${useWebUi}`;
  if (info.version === null) return versionHint;
  if (info.setIp === 'disabled') return 'Changing the IP address from this app is disabled in the camera\'s web interface.';
  return '';
}

function updateStatus(): void {
  const counts: Record<CameraStatus, number> = { 'online': 0, 'other-subnet': 0, 'unreachable': 0, 'offline': 0 };
  for (const camera of cameras.values()) counts[camera.status]++;

  if (cameras.size === 0) {
    statusEl.textContent = '0 cameras found';
    return;
  }
  const parts = [`${counts.online} online`];
  if (counts['other-subnet']) parts.push(`${counts['other-subnet']} not reachable`);
  if (counts.unreachable) parts.push(`${counts.unreachable} not responding`);
  if (counts.offline) parts.push(`${counts.offline} offline`);
  statusEl.textContent = parts.join(', ');
}

/** Escape for HTML text and for double-quoted attribute values. */
function escapeHtml(str: string): string {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML.replace(/"/g, '&quot;');
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;
function toast(message: string, kind: 'info' | 'error' = 'info', duration = 4000): void {
  toastEl.textContent = message;
  toastEl.className = kind;
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.add('hidden'), duration);
}

// Cameras whose identify request is running or whose LED is flashing. Kept
// outside the DOM because render() rebuilds the rows on every update.
const identifying = new Set<string>();

async function identify(id: string): Promise<void> {
  if (identifying.has(id)) return;
  identifying.add(id);
  render();
  let res: IdentifyResult;
  try {
    res = await api.identify(id);
  } catch (err) {
    res = { ok: false, message: `Unexpected error: ${(err as Error).message}` };
  }
  toast(res.message ?? (res.ok ? 'Identifying.' : 'Identify failed.'), res.ok ? 'info' : 'error');

  // Keep the icon flashing as long as the LED does
  const duration = res.ok ? (res.durationS ?? 0) * 1000 : 0;
  setTimeout(() => {
    identifying.delete(id);
    render();
  }, duration);
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
const dlgBody = $('dlg-body');
const dlgMessage = $('dlg-message');
const applyBtn = $<HTMLButtonElement>('dlg-apply');
const defaultBtn = $<HTMLButtonElement>('dlg-default');
const cancelBtn = $<HTMLButtonElement>('dlg-cancel');
const openBtn = $<HTMLButtonElement>('dlg-open');
const staticFields = $('static-fields');
const dhcpFields = $('dhcp-fields');
const fAddress = $<HTMLInputElement>('f-address');
const fGateway = $<HTMLInputElement>('f-gateway');
const fFbAddress = $<HTMLInputElement>('f-fb-address');
const fFbGateway = $<HTMLInputElement>('f-fb-gateway');
const fDns = $<HTMLInputElement>('f-dns');
const suggestionBox = $('suggestion');
const suggestionText = $('suggestion-text');
const suggestionBtn = $<HTMLButtonElement>('suggestion-apply');

interface DialogState {
  cameraId: string;
  fingerprint: string;
  // The device consumes the challenge on every set_ip; fetch a new one before retrying
  challengeUsed: boolean;
  done: boolean;
  touched: boolean;
  // After a successful set_ip: waiting for the camera to show up again
  awaiting: { since: number; address: string | null; timer: ReturnType<typeof setTimeout> } | null;
}

// The rediscovery after set_ip runs after 3 s; earlier updates still show the old state
const REDISCOVERY_DELAY = 2500;
const AWAIT_TIMEOUT = 20_000;

let state: DialogState | null = null;

type FormMode = 'static' | 'dhcp';

function modeValue(): FormMode {
  return (form.querySelector('input[name="mode"]:checked') as HTMLInputElement).value as FormMode;
}

function setMode(mode: FormMode): void {
  (form.querySelector(`input[name="mode"][value="${mode}"]`) as HTMLInputElement).checked = true;
  staticFields.classList.toggle('hidden', mode !== 'static');
  dhcpFields.classList.toggle('hidden', mode !== 'dhcp');
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
  setMode(config.mode === 'static' ? 'static' : 'dhcp');
  // Static fields also hold the current address in DHCP mode, so switching
  // to "Static" starts from what the camera has now.
  fAddress.value = config.address ?? '';
  fGateway.value = config.gateway ?? '';
  fFbAddress.value = config.fallback?.address ?? '';
  fFbGateway.value = config.fallback?.gateway ?? '';
  fDns.value = (config.dns ?? []).join(', ');
}

function describeConfig(config: NetConfig): string {
  if (config.mode === 'default') return 'factory default';
  return config.mode === 'static' ? `static ${config.address}` : 'DHCP';
}

/** Full description of the settings about to be sent, for the login dialog. */
function describeNewConfig(config: NetConfig): string {
  const dns = config.dns?.length ? `, DNS ${config.dns.join(', ')}` : '';
  if (config.mode === 'static') {
    return `static ${config.address}${config.gateway ? `, gateway ${config.gateway}` : ''}${dns}`;
  }
  return `DHCP${config.fallback ? `, fallback ${config.fallback.address}` : ''}${dns}`;
}

function describeCurrent(net: NetState): string {
  return `Current address: ${net.address ?? 'none'}`;
}

/** Offer the suggested config when the camera cannot be reached from this PC. */
function renderSuggestion(camera: Camera, suggestion: NetConfig | undefined): void {
  const info = camera.fdp!;
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
  const local = camera?.fdp?.localAddress;
  if (config.mode === 'static' && local && stripPrefix(config.address ?? null) === local) {
    return 'This is the address of your PC. Choose a different one.';
  }
  return null;
}

function updateApply(): void {
  if (!state) return;
  const err = configError();
  applyBtn.disabled = state.done || !!err;
  if (state.touched && err) {
    showMessage(err, 'error');
    showingValidation = true;
  } else if (showingValidation) {
    hideMessage();
  }
}

/** Nothing more to apply in this dialog: only Close (and Open) remain. */
function endForm(): void {
  if (!state) return;
  state.done = true;
  dlgBody.classList.add('hidden');
  applyBtn.classList.add('hidden');
  defaultBtn.classList.add('hidden');
  cancelBtn.textContent = 'Close';
}

function syncDialogCamera(camera: Camera): void {
  if (!state || state.cameraId !== camera.id || !camera.fdp) return;
  dlgCurrent.textContent = describeCurrent(camera.fdp.net);

  // After set_ip: report once the camera answers with its new settings
  const wait = state.awaiting;
  if (!wait || Date.now() - wait.since < REDISCOVERY_DELAY) return;
  const address = stripPrefix(camera.fdp.net.address ?? null);
  if (wait.address && address !== wait.address) return;
  if (camera.online) {
    finishWaiting();
    showMessage(`Done. The camera is reachable at ${address}.`, 'success');
    openBtn.classList.remove('hidden');
    openBtn.focus();
  } else if (camera.status === 'other-subnet' || camera.status === 'unreachable') {
    // The status comes from a failed web UI check. Keep waiting: the camera
    // may still be starting its web interface.
    const now = camera.fdp.net.address ?? 'no address';
    showMessage(camera.status === 'other-subnet'
      ? `IP configuration applied. The camera now uses ${now}. Its web interface does not respond from this PC; `
        + `the address is outside this PC's network (${camera.fdp.localCidr}) and no route leads there.`
      : `IP configuration applied. The camera now uses ${now}, but its web interface does not respond yet.`, 'info');
  }
}

function finishWaiting(): void {
  if (!state?.awaiting) return;
  clearTimeout(state.awaiting.timer);
  state.awaiting = null;
}

// --- Trust on first use (spec §8.5): own dialog before "Change IP" ---

const trustDialog = $<HTMLDialogElement>('trust-dialog');
const trustAccept = $<HTMLButtonElement>('trust-accept');
const trustCancel = $<HTMLButtonElement>('trust-cancel');

/** Ask whether to trust a new or changed camera key. Resolves true if the user trusts it. */
function confirmTrust(camera: Camera, res: PrepareSetIpResult): Promise<boolean> {
  const info = camera.fdp!;
  const changed = res.trust === 'mismatch';
  const fp = escapeHtml(res.fingerprint ?? '');

  trustDialog.classList.toggle('danger', changed);
  $('trust-title').textContent = changed ? 'Camera Key Changed' : 'New Camera';
  $('trust-camera').textContent = `${camera.hostname} · S/N ${info.serial} · ${info.mac}`;
  $('trust-body').innerHTML = changed
    ? `<p class="trust-text">The key of this camera differs from the one you trusted before. This is expected
        after a factory reset of the camera. Otherwise, another device on the network may be pretending to be
        this camera.</p>
      <p class="trust-text"><strong>Only continue if you know that the camera was reset.</strong></p>
      <div class="trust-box danger">
        <p>Previously trusted:</p><code>${escapeHtml(res.pinnedFingerprint ?? '')}</code>
        <p>Now:</p><code>${fp}</code>
      </div>`
    : `<p class="trust-text">This PC has not changed settings on this camera before. The app remembers the
        camera's key and warns you if it ever changes, for example if another device on the network pretends to
        be this camera.</p>
      <div class="trust-box info"><p>Key fingerprint:</p><code>${fp}</code></div>`;
  trustAccept.textContent = changed ? 'Trust new key' : 'Trust camera';
  trustAccept.className = changed ? 'btn danger' : 'btn primary';

  trustDialog.showModal();
  // A changed key is suspicious: make Cancel the default
  (changed ? trustCancel : trustAccept).focus();

  return new Promise((resolve) => {
    const finish = (trusted: boolean) => {
      trustAccept.onclick = trustCancel.onclick = trustDialog.oncancel = null;
      trustDialog.close();
      resolve(trusted);
    };
    trustAccept.onclick = () => finish(true);
    trustCancel.onclick = () => finish(false);
    trustDialog.oncancel = (e) => {
      e.preventDefault();
      finish(false);
    };
  });
}

// --- Start of the "Change IP" flow ---

// Camera whose challenge is being requested; its button shows "Contacting..."
let preparingId: string | null = null;

async function startChangeIp(id: string): Promise<void> {
  if (preparingId || !cameras.get(id)?.fdp) return;
  preparingId = id;
  render();
  let res: PrepareSetIpResult;
  try {
    res = await api.prepareSetIp(id);
  } catch (err) {
    res = { ok: false, error: `Unexpected error: ${(err as Error).message}` };
  } finally {
    preparingId = null;
    render();
  }

  const camera = cameras.get(id);
  if (!camera?.fdp) return;
  if (res.ok && res.trust !== 'match') {
    if (!(await confirmTrust(camera, res))) return;
    if (!(await api.trustCamera(id))) {
      toast('The camera key could not be stored. Please try again.', 'error');
      return;
    }
  }
  openSetIpDialog(camera, res);
}

function openSetIpDialog(camera: Camera, res: PrepareSetIpResult): void {
  const info = camera.fdp!;
  state = { cameraId: camera.id, fingerprint: '', challengeUsed: false, done: false, touched: false, awaiting: null };
  dlgCamera.textContent = camera.hostname;
  dlgCurrent.textContent = describeCurrent(info.net);
  hideMessage();
  dlgBody.classList.remove('hidden');
  applyBtn.classList.remove('hidden');
  defaultBtn.classList.remove('hidden');
  openBtn.classList.add('hidden');
  cancelBtn.textContent = 'Cancel';

  if (!res.ok) {
    endForm();
    showMessage(res.error ?? 'The camera could not be contacted.', 'error');
    dialog.showModal();
    return;
  }

  state.fingerprint = res.fingerprint!;
  fillConfig(currentConfig(info.net));
  renderSuggestion(camera, res.suggestion);
  updateApply();
  dialog.showModal();
  dialog.scrollTop = 0;
}

/** Get a fresh challenge before a retry. */
async function renewChallenge(): Promise<'ok' | 'key-changed' | { error: string }> {
  const res = await api.prepareSetIp(state!.cameraId);
  if (!res.ok) return { error: res.error ?? 'The camera could not be contacted.' };
  // Nothing is sent to a key the user has not confirmed
  if (res.fingerprint !== state!.fingerprint || res.trust !== 'match') return 'key-changed';
  state!.challengeUsed = false;
  return 'ok';
}

/** set_ip succeeded: wait for the camera to show up with its new settings. */
function applied(res: SetIpResult, config: NetConfig): void {
  if (!state) return;
  endForm();
  const address = res.net?.address ?? (config.mode === 'static' ? config.address : null);
  showMessage(`IP configuration applied${address ? ` (${address})` : ''}. Searching for the camera...`, 'info');
  const id = state.cameraId;
  state.awaiting = {
    since: Date.now(),
    address: stripPrefix(address ?? null),
    timer: setTimeout(() => {
      if (state?.cameraId !== id || !state.awaiting) return;
      state.awaiting = null;
      showMessage(`IP configuration applied${address ? ` (${address})` : ''}. The camera does not answer at its `
        + 'new address yet. It may need a moment; the list updates automatically.', 'info');
    }, AWAIT_TIMEOUT),
  };
}

function closeDialog(): void {
  if (loginBusy) return;
  closeLogin();
  finishWaiting();
  state = null;
  dialog.close();
}

// --- Login dialog: credentials for the settings chosen above ---

const loginDialog = $<HTMLDialogElement>('login-dialog');
const loginForm = $<HTMLFormElement>('login-form');
const loginSummary = $('login-summary');
const loginMessage = $('login-message');
const loginApply = $<HTMLButtonElement>('login-apply');
const loginCancel = $<HTMLButtonElement>('login-cancel');
const fUser = $<HTMLInputElement>('f-user');
const fPassword = $<HTMLInputElement>('f-password');

// Settings the login applies; null while the login dialog is closed
let pendingConfig: NetConfig | null = null;
let loginBusy = false;

function credentialsMissing(): boolean {
  return !fUser.value.trim() || !fPassword.value;
}

function updateLoginApply(): void {
  loginApply.disabled = loginBusy || credentialsMissing();
}

function showLoginMessage(text: string): void {
  loginMessage.textContent = text;
  loginMessage.className = 'dlg-message error';
}

function openLogin(config: NetConfig): void {
  if (!state || state.done) return;
  const camera = cameras.get(state.cameraId);
  pendingConfig = config;
  const reset = config.mode === 'default';
  $('login-camera').textContent = camera?.hostname ?? '';
  loginSummary.textContent = reset
    ? 'Reset the network settings of the camera to its factory defaults (usually DHCP with a fallback address).'
    : `New settings: ${describeNewConfig(config)}`;
  loginApply.textContent = reset ? 'Reset' : 'Apply';
  loginMessage.className = 'dlg-message hidden';
  fPassword.value = '';
  updateLoginApply();
  loginDialog.showModal();
  (fUser.value.trim() ? fPassword : fUser).focus();
}

function closeLogin(): void {
  if (loginBusy) return;
  pendingConfig = null;
  fPassword.value = '';
  if (loginDialog.open) loginDialog.close();
}

function setLoginBusy(busy: boolean): void {
  loginBusy = busy;
  for (const el of loginForm.querySelectorAll<HTMLInputElement>('input')) el.disabled = busy;
  loginCancel.disabled = busy;
  loginApply.textContent = busy ? 'Applying...' : pendingConfig?.mode === 'default' ? 'Reset' : 'Apply';
  updateLoginApply();
}

async function submitLogin(): Promise<void> {
  if (!state || !pendingConfig || loginBusy || credentialsMissing()) return;
  const config = pendingConfig;
  setLoginBusy(true);
  loginMessage.className = 'dlg-message hidden';
  try {
    if (state.challengeUsed) {
      const renewed = await renewChallenge();
      if (renewed === 'key-changed') {
        setLoginBusy(false);
        closeLogin();
        endForm();
        showMessage('The camera key changed. Close this dialog and click "Change IP" again to check the new key.', 'error');
        return;
      }
      if (renewed !== 'ok') {
        showLoginMessage(renewed.error);
        return;
      }
    }

    const res = await api.setIp({ id: state.cameraId, config, user: fUser.value.trim(), password: fPassword.value });
    state.challengeUsed = true;
    if (res.status === 'ok') {
      setLoginBusy(false);
      closeLogin();
      applied(res, config);
      return;
    }
    showLoginMessage(res.message ?? statusMessage(res.status));
    if (res.status === 'auth_failed') fPassword.value = '';
  } catch (err) {
    showLoginMessage(`Unexpected error: ${(err as Error).message}`);
  } finally {
    if (loginBusy) setLoginBusy(false);
    if (loginDialog.open) fPassword.focus();
  }
}

// --- Event wiring ---

form.addEventListener('submit', (e) => {
  e.preventDefault();
  if (!state || state.done) return;
  state.touched = true;
  updateApply();
  if (!configError()) openLogin(readConfig());
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
defaultBtn.addEventListener('click', () => openLogin({ mode: 'default' }));
cancelBtn.addEventListener('click', closeDialog);
openBtn.addEventListener('click', () => {
  if (state) api.openCamera(state.cameraId);
  closeDialog();
});
dialog.addEventListener('cancel', (e) => {
  e.preventDefault();
  closeDialog();
});

loginForm.addEventListener('submit', (e) => {
  e.preventDefault();
  submitLogin();
});
loginForm.addEventListener('input', updateLoginApply);
loginCancel.addEventListener('click', closeLogin);
loginDialog.addEventListener('cancel', (e) => {
  e.preventDefault();
  closeLogin();
});
