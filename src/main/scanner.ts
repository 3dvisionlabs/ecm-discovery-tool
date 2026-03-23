import { EventEmitter } from 'events';
import { spawn } from 'child_process';
import * as dgram from 'dgram';
import * as dns from 'dns';
import * as net from 'net';
import * as dnsPacket from 'dns-packet';
import Bonjour, { type Service } from 'bonjour-service';
import { Camera } from '../shared/types';

const MDNS_MULTICAST = '224.0.0.251';
const MDNS_PORT = 5353;

// How often to run a discovery scan (ms)
const DISCOVERY_INTERVAL = 30_000;
// How often to check camera health via TCP connect (ms)
const HEALTH_CHECK_INTERVAL = 10_000;
// TCP connect timeout per camera (ms)
const TCP_TIMEOUT = 3_000;
// How long to collect mDNS browse results before resolving (ms)
const BROWSE_DURATION = 5_000;

export class Scanner extends EventEmitter {
  private cameras: Map<string, Camera> = new Map();
  // Cameras dismissed by refresh — only re-shown if they pass a TCP check
  private dismissed: Set<string> = new Set();
  private refreshTimeout: ReturnType<typeof setTimeout> | null = null;
  private healthTimer: ReturnType<typeof setInterval> | null = null;
  private discoveryTimer: ReturnType<typeof setInterval> | null = null;
  private bonjourInstance: InstanceType<typeof Bonjour> | null = null;

  start(): void {
    this.stopTimers();
    this.runDiscovery();
    this.healthTimer = setInterval(() => this.checkAllCameras(), HEALTH_CHECK_INTERVAL);
    this.discoveryTimer = setInterval(() => this.runDiscovery(), DISCOVERY_INTERVAL);
  }

  stop(): void {
    if (this.refreshTimeout) {
      clearTimeout(this.refreshTimeout);
      this.refreshTimeout = null;
    }
    this.stopTimers();
    if (this.bonjourInstance) {
      this.bonjourInstance.destroy();
      this.bonjourInstance = null;
    }
    this.cameras.clear();
    this.dismissed.clear();
  }

  refresh(): void {
    if (this.refreshTimeout) return;

    // Dismiss offline cameras — they won't reappear unless TCP check passes
    for (const [id, camera] of this.cameras) {
      if (!camera.online) {
        this.dismissed.add(id);
        this.cameras.delete(id);
      }
    }

    this.runDiscovery();
    this.checkAllCameras();

    this.refreshTimeout = setTimeout(() => {
      this.refreshTimeout = null;
    }, 2000);
  }

  getAll(): Camera[] {
    return Array.from(this.cameras.values());
  }

  getById(id: string): Camera | undefined {
    return this.cameras.get(id);
  }

  private stopTimers(): void {
    if (this.healthTimer) {
      clearInterval(this.healthTimer);
      this.healthTimer = null;
    }
    if (this.discoveryTimer) {
      clearInterval(this.discoveryTimer);
      this.discoveryTimer = null;
    }
  }

  // --- Discovery via native OS mDNS tools ---

  private runDiscovery(): void {
    if (process.platform === 'darwin') {
      this.discoverDarwin();
    } else if (process.platform === 'win32') {
      this.discoverWindows();
    } else {
      this.discoverLinux();
    }
  }

  /**
   * macOS: use dns-sd -B to browse, then dns.lookup() to resolve IPs.
   * dns-sd -B streams continuously, so we kill it after BROWSE_DURATION.
   * The instance name for SSH services equals the hostname.
   */
  private discoverDarwin(): void {
    const instances = new Set<string>();
    const proc = spawn('dns-sd', ['-B', '_ssh._tcp', 'local.']);

    proc.on('error', (err: NodeJS.ErrnoException) => {
      console.error('dns-sd error:', err.message);
    });

    proc.stdout.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n')) {
        // Lines look like: "15:20:25.880  Add  3  25 local.  _ssh._tcp.  ecm-232250000988"
        const match = line.match(/\bAdd\b.*_ssh\._tcp\.\s+(.+)$/);
        if (!match) continue;
        const hostname = match[1].trim();
        if (!hostname.toLowerCase().startsWith('ecm-')) continue;
        if (instances.has(hostname)) continue;
        instances.add(hostname);
        this.resolveAndAdd(hostname);
      }
    });

    setTimeout(() => {
      proc.kill();
    }, BROWSE_DURATION);
  }

  /**
   * Linux: use avahi-browse for one-shot discovery with resolved addresses.
   */
  private discoverLinux(): void {
    const proc = spawn('avahi-browse', ['-r', '-p', '-t', '_ssh._tcp']);

    proc.on('error', (err: NodeJS.ErrnoException) => {
      if (err.code === 'ENOENT') {
        console.error('avahi-browse not found. Install avahi-utils (e.g., sudo apt install avahi-utils).');
      } else {
        console.error('avahi-browse error:', err.message);
      }
    });

    proc.stdout.on('data', (data: Buffer) => {
      for (const line of data.toString().split('\n')) {
        // Parseable format: =;iface;protocol;name;type;domain;hostname;address;port;txt
        const fields = line.split(';');
        if (fields[0] !== '=') continue;

        const instance = fields[3] || '';
        if (!instance.toLowerCase().startsWith('ecm-')) continue;

        const hostname = (fields[6] || '').replace(/\.local\.?$/, '');
        const address = fields[7] || '';

        // Use the resolved address from avahi if it's IPv4, otherwise fall back to dns.lookup
        if (hostname && /^\d+\.\d+\.\d+\.\d+$/.test(address)) {
          this.addCamera(hostname, address);
        } else if (hostname) {
          this.resolveAndAdd(hostname);
        }
      }
    });
  }

  /**
   * Windows: send a raw mDNS PTR query for _ssh._tcp.local via UDP multicast.
   * Uses an ephemeral source port (not 5353), so responses come back as unicast
   * per RFC 6762 §5.5. This avoids binding to port 5353 and conflicts with
   * other mDNS tools (e.g. Python zeroconf) running on the same machine.
   */
  private discoverWindows(): void {
    const query = dnsPacket.encode({
      type: 'query',
      id: 0,
      flags: 0,
      questions: [{ type: 'PTR', name: '_ssh._tcp.local', class: 'IN' }],
    });

    const sock = dgram.createSocket({ type: 'udp4', reuseAddr: false });

    sock.on('message', (msg: Buffer) => {
      try {
        const response = dnsPacket.decode(msg);
        this.handleMdnsResponse(response);
      } catch {
        // Ignore malformed packets
      }
    });

    sock.on('error', (err) => {
      console.error('mDNS UDP error:', err.message);
      sock.close();
    });

    // Bind to ephemeral port, then send query
    sock.bind(0, () => {
      sock.send(query, 0, query.length, MDNS_PORT, MDNS_MULTICAST, (err) => {
        if (err) console.error('mDNS send error:', err.message);
      });

      // Close socket after browse duration
      setTimeout(() => {
        try { sock.close(); } catch { /* already closed */ }
      }, BROWSE_DURATION);
    });
  }

  /**
   * Parse an mDNS response: extract PTR → hostname, A → IP address.
   * A single response may contain multiple answers and additional records.
   */
  private handleMdnsResponse(response: dnsPacket.DecodedPacket): void {
    // Collect A records from answers + additionals for hostname→IP lookup
    const aRecords = new Map<string, string>();
    const allRecords = [...(response.answers || []), ...(response.additionals || [])];

    for (const rec of allRecords) {
      if (rec.type === 'A' && typeof rec.data === 'string') {
        // hostname.local → IP
        const name = rec.name.replace(/\.local\.?$/, '');
        aRecords.set(name.toLowerCase(), rec.data);
      }
    }

    // Process PTR records to find service instances
    for (const rec of allRecords) {
      if (rec.type !== 'PTR' || typeof rec.data !== 'string') continue;
      // PTR data looks like "ecm-12345678._ssh._tcp.local"
      const instanceName = rec.data.replace(/\._ssh\._tcp\.local\.?$/, '');
      if (!instanceName.toLowerCase().startsWith('ecm-')) continue;

      // Try to find an A record for this hostname
      const ip = aRecords.get(instanceName.toLowerCase());
      if (ip) {
        this.addCamera(instanceName, ip);
      } else {
        // Fallback: resolve via OS DNS
        this.resolveAndAdd(instanceName);
      }
    }
  }

  /**
   * Resolve a hostname via the OS DNS/mDNS resolver and add it as a camera.
   */
  private resolveAndAdd(hostname: string): void {
    dns.lookup(`${hostname}.local`, { family: 4 }, (err, address) => {
      if (!err && address) {
        this.addCamera(hostname, address);
      }
    });
  }

  private addCamera(hostname: string, ip: string): void {
    const id = `${hostname}-${ip}`;
    const existing = this.cameras.get(id);

    if (existing) {
      // Already known — just verify it's still reachable
      this.checkCamera(existing);
      return;
    }

    if (this.dismissed.has(id)) {
      // Dismissed by refresh — silently check if it's back online
      const camera: Camera = { id, hostname, ip, port: 443, lastSeen: Date.now(), online: false };
      this.checkCameraDismissed(camera);
      return;
    }

    // New camera — notify renderer and verify status via TCP
    const camera: Camera = {
      id,
      hostname,
      ip,
      port: 443,
      lastSeen: Date.now(),
      online: false,
    };
    this.cameras.set(id, camera);
    this.emit('camera-found', camera);
    this.checkCamera(camera);
  }

  /**
   * Check a dismissed camera — only re-add if it's actually reachable.
   */
  private checkCameraDismissed(camera: Camera): void {
    const socket = new net.Socket();
    socket.setTimeout(TCP_TIMEOUT);

    socket.on('connect', () => {
      socket.destroy();
      // Camera is back online — un-dismiss and show it
      this.dismissed.delete(camera.id);
      camera.online = true;
      camera.lastSeen = Date.now();
      this.cameras.set(camera.id, camera);
      this.emit('camera-found', camera);
    });

    socket.on('timeout', () => socket.destroy());
    socket.on('error', () => socket.destroy());

    socket.connect(camera.port, camera.ip);
  }

  // --- TCP health checks ---

  private checkAllCameras(): void {
    for (const camera of this.cameras.values()) {
      this.checkCamera(camera);
    }
  }

  private checkCamera(camera: Camera): void {
    const socket = new net.Socket();

    socket.setTimeout(TCP_TIMEOUT);

    socket.on('connect', () => {
      socket.destroy();
      if (!camera.online) {
        camera.online = true;
        camera.lastSeen = Date.now();
        this.emit('camera-found', camera);
      } else {
        camera.lastSeen = Date.now();
      }
    });

    socket.on('timeout', () => {
      socket.destroy();
      if (camera.online) {
        camera.online = false;
        this.emit('camera-lost', camera);
      }
    });

    socket.on('error', () => {
      socket.destroy();
      if (camera.online) {
        camera.online = false;
        this.emit('camera-lost', camera);
      }
    });

    socket.connect(camera.port, camera.ip);
  }
}
