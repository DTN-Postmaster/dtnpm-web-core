import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.resolve(__dirname, '..');
const ROOT_DIR = path.resolve(UI_DIR, '../..');
const PORT = 5179;
const BASE_URL = `http://127.0.0.1:${PORT}`;

const DESTS = {
  webCore: path.join(ROOT_DIR, 'dtnpm-web-core/docs/screenshots'),
  chat: path.join(ROOT_DIR, 'dtnpm-mod-chat/docs/screenshots'),
  files: path.join(ROOT_DIR, 'dtnpm-mod-files/docs/screenshots'),
  senml: path.join(ROOT_DIR, 'dtnpm-mod-senml/docs/screenshots'),
  rego: path.join(ROOT_DIR, 'dtnpm-mod-rego-routing/docs/screenshots'),
  dtnpm: path.join(ROOT_DIR, 'dtnpm/docs/screenshots'),
  orgProfile: path.join(ROOT_DIR, '.github/profile/screenshots'),
};

for (const dir of Object.values(DESTS)) {
  fs.mkdirSync(dir, { recursive: true });
}

function waitForServer(url, timeoutMs = 20000) {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    const check = async () => {
      try {
        const res = await fetch(url);
        if (res.ok) return resolve();
      } catch {
        // retry
      }
      if (Date.now() - start > timeoutMs) {
        return reject(new Error(`Timed out waiting for Vite server at ${url}`));
      }
      setTimeout(check, 250);
    };
    check();
  });
}

const mockConfig = {
  status: 'running',
  version: '0.2.0',
  callsign: 'F4JXQ-2',
  node_id: 'dtn://f4jxq-2/',
  admin_endpoints: ['dtn://f4jxq-2/~admin'],
  storage: {
    in_memory: false,
    sqlite_path: '/var/lib/dtn-hdy-ham/bundles.db',
  },
  grpc: { enabled: true, address: '127.0.0.1:50051' },
  web: { address: '0.0.0.0:8088', static_dir: './web/dist' },
  clas: [
    {
      name: 'tcp-f4jxq-gw',
      type: 'tcpclv4',
      connect: '44.27.131.233:4556',
      peers: ['dtn://f4jxq-gw/'],
      connected: true,
    },
    {
      name: 'mercury-hf',
      type: 'mercury',
      port: '/dev/ttyUSB0',
      baud: 115200,
      peers: ['dtn://va3nb/'],
      connected: true,
    },
    {
      name: 'hqfbp-uhf',
      type: 'hqfbp',
      port: '127.0.0.1:8001',
      peers: ['dtn://ve3yca/'],
      connected: false,
    },
  ],
  routes: [
    { pattern: 'dtn://f4jxq-2/*', action: 'local', priority: 1 },
    { pattern: 'dtn://va3nb/**', action: 'via', next_hop: 'dtn://va3nb/', priority: 10 },
    { pattern: 'dtn://**', action: 'via', next_hop: 'dtn://f4jxq-gw/', priority: 100 },
  ],
};

const mockServices = [
  { eid: 'dtn://f4jxq-2/echo', type: 'Built-in Echo', active: true },
  { eid: 'dtn://f4jxq-2/chat', type: 'DTN Chat', active: true },
  { eid: 'dtn://f4jxq-2/files', type: 'DTN Files', active: true },
  { eid: 'dtn://f4jxq-2/senml', type: 'SenML Telemetry', active: true },
];

const mockBundles = [
  {
    id: '[dtn://f4jxq-2/chat, 1763400120, 1]',
    source: 'dtn://f4jxq-2/chat',
    destination: 'dtn://va3nb/chat',
    status: 'ForwardPending',
    creation_time: 1763400120,
    created_at: '2026-11-17T18:15:20Z',
    lifetime_secs: 86400,
    size: 266,
    auth_status: 'valid',
  },
  {
    id: '[dtn://ve3yca/senml, 1763400080, 4]',
    source: 'dtn://ve3yca/senml',
    destination: 'dtn://f4jxq-2/senml',
    status: 'Delivered',
    creation_time: 1763400080,
    created_at: '2026-11-17T18:14:40Z',
    lifetime_secs: 3600,
    size: 416,
    auth_status: 'valid',
  },
  {
    id: '[dtn://va3nb/files, 1763399900, 2]',
    source: 'dtn://va3nb/files',
    destination: 'dtn://f4jxq-2/files',
    status: 'Waiting',
    creation_time: 1763399900,
    created_at: '2026-11-17T18:11:40Z',
    lifetime_secs: 172800,
    size: 18516,
    auth_status: 'unsigned',
  },
];

const mockConversations = [
  {
    peer: 'dtn://va3nb/chat',
    callsign: 'VA3NB',
    last_message: '73 de VA3NB! Copy 599 via 40m Mercury ARQ, wx clear.',
    last_direction: 'inbound',
    last_status: 'received',
    last_timestamp: 1763400900000,
    last_created_at: '2026-11-17T18:15:00Z',
    unread_count: 1,
    total_messages: 4,
  },
  {
    peer: 'dtn://ve3yca/chat',
    callsign: 'VE3YCA',
    last_message: 'APRS trajectory bundle relayed through F4JXQ-GW.',
    last_direction: 'outbound',
    last_status: 'sent',
    last_timestamp: 1763399000000,
    last_created_at: '2026-11-17T17:43:20Z',
    unread_count: 0,
    total_messages: 3,
  },
];

const mockChatMessages = [
  {
    id: 'chat-msg-1',
    conversation_peer: 'dtn://va3nb/chat',
    sender_eid: 'dtn://f4jxq-2/chat',
    recipient_eid: 'dtn://va3nb/chat',
    direction: 'outbound',
    content: 'CQ VA3NB de F4JXQ-2, testing **BPv7 + BIB** over Mercury HF ARQ.',
    status: 'sent',
    is_read: true,
    created_at: '2026-11-17T18:10:00Z',
    timestamp: Date.now() - 300_000,
    auth_status: 'valid',
  },
  {
    id: 'chat-msg-2',
    conversation_peer: 'dtn://va3nb/chat',
    sender_eid: 'dtn://va3nb/chat',
    recipient_eid: 'dtn://f4jxq-2/chat',
    direction: 'inbound',
    content: '73 de VA3NB! Copy 599 via 40m Mercury ARQ, wx clear.',
    status: 'received',
    is_read: false,
    created_at: '2026-11-17T18:15:00Z',
    timestamp: Date.now() - 60_000,
    auth_status: 'valid',
  },
];

const mockFiles = [
  {
    id: 'file-001',
    filename: 'emcom-sitrep-sector4.md',
    mime_type: 'text/markdown',
    size_bytes: 2480,
    direction: 'inbound',
    sender: 'dtn://va3nb/files',
    recipient: 'dtn://f4jxq-2/files',
    created_at: '2026-11-17T18:12:00Z',
    auth_status: 'valid',
    read: false,
    preview_text: '# EmComm Sector 4 SITREP\n\n- **Power**: Solar + LiFePO4 94%\n- **HF Link**: 7.065 MHz Mercury ARQ active\n- **VHF**: APRS digipeater operational',
  },
  {
    id: 'file-002',
    filename: 'repeater-telemetry.json',
    mime_type: 'application/json',
    size_bytes: 1024,
    direction: 'outbound',
    sender: 'dtn://f4jxq-2/files',
    recipient: 'dtn://ve3yca/files',
    created_at: '2026-11-17T17:50:00Z',
    auth_status: 'valid',
    read: true,
  },
];

const NOW_MS = Date.now();
const mockSenmlCards = [
  {
    id: 1,
    name: 'battery_voltage',
    unit: 'V',
    custom_label: 'Station Battery',
    display_order: 1,
    is_visible: true,
    utc: NOW_MS - 120_000,
    value: 13.6,
    str_value: null,
  },
  {
    id: 2,
    name: 'solar_current',
    unit: 'A',
    custom_label: 'Solar Array',
    display_order: 2,
    is_visible: true,
    utc: NOW_MS - 120_000,
    value: 4.8,
    str_value: null,
  },
  {
    id: 3,
    name: 'pa_temp',
    unit: 'Cel',
    custom_label: 'PA Heatsink Temp',
    display_order: 3,
    is_visible: true,
    utc: NOW_MS - 120_000,
    value: 34.2,
    str_value: null,
  },
  {
    id: 4,
    name: 'hf_snr',
    unit: 'dB',
    custom_label: 'Mercury 40m SNR',
    display_order: 4,
    is_visible: true,
    utc: NOW_MS - 120_000,
    value: 14.5,
    str_value: null,
  },
];

const mockRegoBlacklist = [
  {
    id: 'bl-1',
    eid_pattern: 'dtn://spam-node/**',
    reason: 'Flooding guard',
    enabled: true,
    created_at: 1763390000,
    created_by: 'admin',
  },
];

const mockRegoPolicy = {
  config: {
    local_eid: 'dtn://f4jxq-2/',
    node_eids: ['dtn://f4jxq-2/'],
    callsign: 'F4JXQ-2',
    is_digipeater: true,
    supported_aliases: ['WIDE1-1', 'EMCOM'],
    trapped_aliases: [],
    hop_limit: 16,
    treb_block_type: 201,
    record_treb: true,
    active_profile: 'aprs-spray-and-wait',
    custom_ingress_rego: null,
    custom_egress_rego: null,
  },
  sources: {
    constants: 'package dtn.constants\nblock_type_hop_count := 10\nblock_type_mesh_routing := 194\n',
    helpers: 'package dtn.helpers\nimport rego.v1\n',
    ingress: 'package dtn.ingress\nimport rego.v1\ndefault decision := {"action": "ACCEPT"}\n',
    egress: 'package dtn.egress\nimport rego.v1\ndefault decision := {"action": "ALLOW"}\n',
  },
};

const mockRegoDecisions = [
  {
    id: 1,
    timestamp: 1763400900,
    phase: 'rib_decide',
    bundle_id: 'dtn://f4jxq-2/chat#1763400120-1',
    source: 'dtn://f4jxq-2/chat',
    destination: 'dtn://va3nb/chat',
    peer_eid: 'dtn://va3nb/',
    action: 'FORWARD',
    reason: 'APRS strict next-hop match via RegoRib',
    route_key: 'dtn://va3nb/',
  },
  {
    id: 2,
    timestamp: 1763400820,
    phase: 'egress',
    bundle_id: 'dtn://va3nb/files#1763399900-2',
    source: 'dtn://va3nb/files',
    destination: 'dtn://f4jxq-2/files',
    peer_eid: 'dtn://f4jxq-gw/',
    action: 'FORWARD_REPLICATE',
    reason: 'Spray-and-Wait binary split L=8 -> 4',
    route_key: null,
  },
];

async function setupMocks(page) {
  await page.route('**/api/**', async (route) => {
    const url = new URL(route.request().url());
    const p = url.pathname;

    if (p === '/api/capabilities') {
      return route.fulfill({
        json: {
          modules: ['chat', 'files', 'senml', 'rego_routing'],
          auth_enabled: false,
          user: { username: 'f4jxq-2', role: 'admin' },
          capabilities: {
            can_manage_clas: true,
            can_send_bundles: true,
            can_manage_security: true,
          },
        },
      });
    }
    if (p === '/api/v1/config') return route.fulfill({ json: mockConfig });
    if (p === '/api/v1/services') return route.fulfill({ json: mockServices });
    if (p === '/api/v1/bundles') return route.fulfill({ json: mockBundles });
    if (p === '/api/v1/logs') {
      return route.fulfill({
        json: [
          { timestamp: '2026-11-17T18:15:00Z', level: 'INFO', target: 'dtnpm::server', message: 'Connected to TCPCL peer 44.27.131.233:4556' },
          { timestamp: '2026-11-17T18:15:20Z', level: 'INFO', target: 'dtnpm_mod_rego_routing::rib', message: 'RegoRib::decide -> FORWARD dtn://va3nb/' },
        ],
      });
    }
    if (p === '/api/chat/v1/conversations') return route.fulfill({ json: mockConversations });
    if (p === '/api/chat/v1/messages') return route.fulfill({ json: mockChatMessages });
    if (p === '/api/v1/events') {
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' });
    }
    if (p === '/api/files/v1/unread') return route.fulfill({ json: { unread_count: 1 } });
    if (p === '/api/files/v1' || p === '/api/files/v1/messages' || p === '/api/files/v1/inbox') {
      return route.fulfill({
        json: [
          {
            id: '[dtn://va3nb/files, 1000, 0]',
            source_eid: 'dtn://va3nb/files',
            destination_eid: 'dtn://f4jxq-2/files',
            creation_timestamp: 1000,
            lifetime_secs: 86400,
            expiry_timestamp: 1763487300,
            size: 2480,
            payload_path: '/var/lib/dtn-hdy-ham/files/emcom-sitrep-sector4.md',
            mime_type: 'text/markdown',
            is_sticky: true,
            is_read: false,
            created_at: '2026-11-17T18:12:00Z',
          },
          {
            id: '[dtn://ve3yca/files, 2000, 0]',
            source_eid: 'dtn://ve3yca/files',
            destination_eid: 'dtn://f4jxq-2/files',
            creation_timestamp: 2000,
            lifetime_secs: 86400,
            expiry_timestamp: 1763487300,
            size: 1024,
            payload_path: '/var/lib/dtn-hdy-ham/files/repeater-telemetry.json',
            mime_type: 'application/json',
            is_sticky: false,
            is_read: true,
            created_at: '2026-11-17T17:50:00Z',
          },
        ],
      });
    }
    if (p.startsWith('/api/files/v1/messages/') && p.endsWith('/preview')) {
      return route.fulfill({ status: 200, contentType: 'text/markdown', body: mockFiles[0].preview_text });
    }
    if (p === '/api/senml/v1/metrics/latest') return route.fulfill({ json: mockSenmlCards });
    if (p === '/api/senml/v1/metrics') return route.fulfill({ json: mockSenmlCards });
    if (p === '/api/rego_routing/v1/blacklist') return route.fulfill({ json: mockRegoBlacklist });
    if (p === '/api/rego_routing/v1/policy') return route.fulfill({ json: mockRegoPolicy });
    if (p === '/api/rego_routing/v1/decisions') return route.fulfill({ json: mockRegoDecisions });

    return route.fulfill({ json: {} });
  });
}

async function main() {
  const vite = spawn('npx', ['vite', '--host', '127.0.0.1', '--port', String(PORT)], {
    cwd: UI_DIR,
    stdio: 'ignore',
  });

  try {
    await waitForServer(BASE_URL);
    const browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
      serviceWorkers: 'block',
    });
    await setupMocks(context);

    const capture = async (hash, waitSelector, targets, afterNav) => {
      const page = await context.newPage();
      page.on('pageerror', (err) => console.error('PAGEERROR:', err));
      await page.goto(`${BASE_URL}/${hash}`, { waitUntil: 'networkidle' });
      if (waitSelector) {
        await page.waitForSelector(waitSelector, { timeout: 5000 }).catch(() => {});
      }
      if (afterNav) {
        await afterNav(page);
      }
      await page.waitForTimeout(450);
      const buf = await page.screenshot({ fullPage: false });
      await page.close();
      for (const t of targets) {
        fs.writeFileSync(t, buf);
        console.log(`Saved ${path.relative(ROOT_DIR, t)}`);
      }
    };

    await capture('#/station/overview', 'text=F4JXQ-2', [
      path.join(DESTS.webCore, 'station-overview.png'),
      path.join(DESTS.dtnpm, 'station-overview.png'),
      path.join(DESTS.orgProfile, 'station-overview.png'),
    ]);

    await capture('#/station/bundles', 'text=dtn://f4jxq-2/chat', [
      path.join(DESTS.webCore, 'bundles-inspector.png'),
      path.join(DESTS.dtnpm, 'bundles-inspector.png'),
    ]);

    await capture('#/send', 'text=Send Bundle', [
      path.join(DESTS.webCore, 'send-bundle.png'),
    ]);

    await capture(
      '#/chat',
      'text=VA3NB',
      [
        path.join(DESTS.chat, 'chat.png'),
        path.join(DESTS.dtnpm, 'chat.png'),
        path.join(DESTS.orgProfile, 'chat.png'),
      ],
      async (page) => {
        await page.locator('text=VA3NB').first().click().catch(() => {});
        await page.waitForSelector('text=testing', { timeout: 3000 }).catch(() => {});
      },
    );

    await capture('#/files', 'text=dtn://va3nb/files', [
      path.join(DESTS.files, 'files.png'),
      path.join(DESTS.dtnpm, 'files.png'),
    ]);

    await capture('#/senml', 'text=Station Battery', [
      path.join(DESTS.senml, 'senml.png'),
      path.join(DESTS.dtnpm, 'senml.png'),
      path.join(DESTS.orgProfile, 'senml.png'),
    ]);

    await capture('#/rego_routing', 'text=dtn://spam-node/**', [
      path.join(DESTS.rego, 'rego-routing.png'),
      path.join(DESTS.dtnpm, 'rego-routing.png'),
      path.join(DESTS.orgProfile, 'rego-routing.png'),
    ]);

    await browser.close();
  } finally {
    vite.kill('SIGTERM');
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
