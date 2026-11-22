import { test, expect } from '@playwright/test';

const mockStationConfig = {
  status: 'running',
  version: '0.1.0',
  callsign: 'F4JXQ',
  node_id: 'dtn://f4jxq/',
  admin_endpoints: ['dtn://f4jxq/~admin'],
  storage: {
    in_memory: false,
    sqlite_path: '/var/lib/dtnpm/bundles.db',
  },
  grpc: {
    enabled: true,
    address: '127.0.0.1:50051',
  },
  web: {
    address: '0.0.0.0:8080',
    static_dir: './web/dist',
  },
  clas: [
    {
      name: 'tcp-f4jxq',
      type: 'tcpclv4',
      connect: '192.168.1.100:4556',
      peers: ['192.168.1.100:4556'],
    },
    {
      name: 'mercury-kiss',
      type: 'mercury',
      port: '/dev/ttyUSB0',
      baud: 115200,
    },
  ],
  routes: [
    {
      pattern: 'dtn://f4jxq/*',
      cla: 'local',
    },
    {
      pattern: 'dtn://*',
      cla: 'tcp-f4jxq',
    },
  ],
};

test.describe('Station Overview & Identity', () => {
  test.beforeEach(async ({ page }) => {
    // Mock the station config endpoint before navigating
    await page.route('**/api/v1/config', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockStationConfig),
      });
    });

    // Mock active services endpoint
    await page.route('**/api/v1/services', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            eid: 'dtn://f4jxq/echo',
            type: 'Built-in Echo',
            registered_at: '2026-11-04T14:00:00+01:00',
          },
        ]),
      });
    });

    // Mock static routes endpoint
    await page.route('**/api/v1/routes*', async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockStationConfig.routes),
        });
      } else if (route.request().method() === 'POST') {
        const postData = route.request().postDataJSON();
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'created', route: postData }),
        });
      } else if (route.request().method() === 'DELETE') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'deleted' }),
        });
      }
    });

    // Mock CLA peer status endpoint
    await page.route('**/api/v1/clas/status', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          {
            address: '192.168.1.100:4556',
            status: 'connected',
            last_attempt_at: null,
          },
        ]),
      });
    });

    await page.goto('/');
  });

  test('displays station identity, callsign and node EID in navbar', async ({ page }) => {
    const navbar = page.locator('header');
    await expect(navbar).toBeVisible();

    // Verify title and badge
    await expect(navbar.getByText(/DTN Postmaster|Hardy BPA/)).toBeVisible();
    await expect(navbar.getByText('Hamradio')).toBeVisible();

    // Verify amateur radio callsign and node EID
    await expect(navbar.getByText('F4JXQ')).toBeVisible();
    await expect(navbar.getByText('dtn://f4jxq/')).toBeVisible();
  });

  test('renders station overview card, version, and parameters', async ({ page }) => {
    // Station overview title and version
    await expect(page.getByRole('heading', { name: 'Station Overview' })).toBeVisible();
    await expect(page.getByText('v0.1.0')).toBeVisible();

    // Check node parameters in overview
    await expect(page.getByText('dtn://f4jxq/~admin')).toBeVisible();
    await expect(page.getByText('/var/lib/dtnpm/bundles.db')).toBeVisible();
  });

  test('lists active convergence layers and routes', async ({ page }) => {
    // Check CLAs are listed (exact match to avoid matching route card "via tcp-f4jxq")
    await expect(page.getByText('tcp-f4jxq', { exact: true })).toBeVisible();
    await expect(page.getByText('mercury-kiss', { exact: true })).toBeVisible();

    // Check routes are listed in Routing (RIB) sub-tab
    await page.getByRole('button', { name: /Routing \(RIB\)/i }).click();
    await expect(page.getByText('dtn://f4jxq/*', { exact: true })).toBeVisible();
    await expect(page.getByText('dtn://*', { exact: true })).toBeVisible();
  });

  test('triggers manual peer connection with hand button', async ({ page }) => {
    // Intercept POST /api/v1/clas/connect
    await page.route('**/api/v1/clas/connect*', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          status: 'connected',
          peer: '192.168.1.100:4556',
          message: 'Successfully connected to peer 192.168.1.100:4556',
        }),
      });
    });

    // Check hand button exists
    const connectBtn = page.locator('button:has-text("Connect")');
    await expect(connectBtn).toBeVisible();
    await expect(connectBtn).toContainText('✋');

    // Click connect
    await connectBtn.click();

    // Verify success banner appears
    await expect(page.locator('text=Successfully connected to peer 192.168.1.100:4556')).toBeVisible();
  });

  test('scopes connect error banner to the target CLA card only', async ({ page }) => {
    await page.route('**/api/v1/config', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          ...mockStationConfig,
          clas: [
            {
              name: 'tcp-f4jxq',
              type: 'tcpclv4',
              peers: ['192.168.1.100:4556'],
            },
            {
              name: 'mercury-hf',
              type: 'mercury-arq',
              peers: [{ callsign: 'VA3NB' }],
            },
          ],
        }),
      });
    });

    await page.route('**/api/v1/clas/connect*', async (route) => {
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({
          error: 'Connection to VA3NB failed: Connection timed out',
          peer: 'VA3NB',
        }),
      });
    });

    await page.goto('/');

    const mercuryCard = page.locator('div.rounded-xl', { hasText: 'mercury-hf' }).first();
    const tcpCard = page.locator('div.rounded-xl', { hasText: 'tcp-f4jxq' }).first();

    await mercuryCard.getByRole('button', { name: /Connect/i }).click();

    await expect(page.getByText('Connection to VA3NB failed: Connection timed out')).toHaveCount(1);
    await expect(mercuryCard.getByText('Connection to VA3NB failed: Connection timed out')).toBeVisible();
    await expect(tcpCard.getByText('Connection to VA3NB failed: Connection timed out')).not.toBeVisible();
  });

  test('renders active DTN services with permanent echo service', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Active DTN Services' })).toBeVisible();
    await expect(page.getByText('1 active')).toBeVisible();
    await expect(page.getByText('dtn://f4jxq/echo')).toBeVisible();
    await expect(page.getByText('Built-in Echo')).toBeVisible();
    await expect(page.getByText('2026-11-04T14:00:00+01:00')).toBeVisible();
  });

  test('toggles raw JSON configuration viewer', async ({ page }) => {
    const toggleBtn = page.getByRole('button', { name: 'View JSON' });
    await expect(toggleBtn).toBeVisible();

    // Initially raw JSON pre block is not displayed
    await expect(page.locator('pre')).not.toBeVisible();

    // Click to open raw JSON
    await toggleBtn.click();
    await expect(page.getByRole('button', { name: 'Hide JSON' })).toBeVisible();
    await expect(page.locator('pre')).toBeVisible();
    await expect(page.locator('pre')).toContainText('"callsign": "F4JXQ"');

    // Click again to hide
    await page.getByRole('button', { name: 'Hide JSON' }).click();
    await expect(page.locator('pre')).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'View JSON' })).toBeVisible();
  });

  test('toggles dark and light mode theme', async ({ page }) => {
    const html = page.locator('html');
    const themeBtn = page.getByTitle(/Switch to (dark|light) mode/);
    await expect(themeBtn).toBeVisible();

    // Determine initial state
    const isDarkInitially = await html.evaluate((el) => el.classList.contains('dark'));

    // Toggle theme
    await themeBtn.click();
    if (isDarkInitially) {
      await expect(html).not.toHaveClass(/dark/);
    } else {
      await expect(html).toHaveClass(/dark/);
    }

    // Toggle back
    await themeBtn.click();
    if (isDarkInitially) {
      await expect(html).toHaveClass(/dark/);
    } else {
      await expect(html).not.toHaveClass(/dark/);
    }
  });

  test('displays peer connection status badge and allows adding a static route', async ({ page }) => {
    // Check connected status badge is displayed
    await expect(page.getByText('connected', { exact: true })).toBeVisible();

    // Switch to Routing (RIB) sub-tab
    await page.getByRole('button', { name: /Routing \(RIB\)/i }).click();

    // Click "+ Add Route" button
    await page.getByRole('button', { name: '+ Add Route' }).click();

    // Fill in pattern
    await page.locator('input[placeholder*="dtn://f4jxq-9/*"]').fill('dtn://f4jxq-9/*');

    // Click submit "Add Route"
    await page.getByRole('button', { name: 'Add Route', exact: true }).click();

    // Check success notification appears
    await expect(page.getByText(/Route .* successfully added/)).toBeVisible();
  });

  test('displays error message and retry button when config API fails', async ({ page }) => {
    // Override route to return 500 error
    await page.route('**/api/v1/config', async (route) => {
      await route.fulfill({
        status: 500,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Internal Server Error' }),
      });
    });

    await page.goto('/');

    await expect(page.getByText('Error loading configuration')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
  });

  test('navigates across all 7 station sub-tabs via segmented navigation bar and excludes Rego Routing from global navbar', async ({ page }) => {
    await page.route('**/api/capabilities', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ modules: ['chat', 'files', 'senml', 'rego_routing'] }),
      });
    });
    await page.route('**/api/rego_routing/v1/blacklist', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) });
    });
    await page.route('**/api/rego_routing/v1/policy', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          config: {
            local_eid: 'dtn://f4jxq/',
            node_eids: ['dtn://f4jxq/'],
            is_digipeater: false,
            supported_aliases: [],
            trapped_aliases: [],
            hop_limit: 16,
            treb_block_type: 201,
            record_treb: true,
            active_profile: 'default',
          },
          sources: {
            constants: 'package dtn.constants',
            helpers: 'package dtn.helpers',
            ingress: 'package dtn.ingress',
            egress: 'package dtn.egress',
            routing: 'package dtn.routing',
          },
        }),
      });
    });
    await page.route('**/api/rego_routing/v1/decisions', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) });
    });
    // Route for Bundles view when embedded
    await page.route('**/api/v1/bundles*', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) });
    });
    // Route for Logs view when embedded
    await page.route('**/api/v1/logs', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ logs: [], count: 0, capacity: 500 }) });
    });

    await page.goto('/');

    // Rego Routing must NOT appear in the top-level global navbar
    const globalNav = page.locator('header nav');
    await expect(globalNav.getByRole('button', { name: /Rego/i })).toHaveCount(0);

    const subNav = page.locator('nav[aria-label="Station management sub-navigation"]');
    await expect(subNav).toBeVisible();

    // Verify exact order of the 7 Station sub-tabs (Rego Routing between Routing (RIB) and BPSec Security)
    const subButtons = subNav.locator('button');
    await expect(subButtons).toHaveCount(7);
    await expect(subButtons.nth(0)).toContainText('Links & Radio');
    await expect(subButtons.nth(1)).toContainText('Routing (RIB)');
    await expect(subButtons.nth(2)).toContainText('Rego Routing');
    await expect(subButtons.nth(3)).toContainText('BPSec Security');
    await expect(subButtons.nth(4)).toContainText('Storage & Bundles');
    await expect(subButtons.nth(5)).toContainText('Logs & Diagnostics');
    await expect(subButtons.nth(6)).toContainText('JSON Config');

    // 1. Links & Radio (default)
    await expect(page.getByText('Radio Identity')).toBeVisible();

    // 2. Routing (RIB)
    await subNav.getByRole('button', { name: /Routing \(RIB\)/i }).click();
    await expect(page.getByRole('heading', { name: 'Static Routes' })).toBeVisible();

    // 3. Rego Routing
    await subNav.getByRole('button', { name: 'Rego Routing' }).click();
    await expect(page.getByRole('heading', { name: 'OPA / Rego Routing & Filtering' })).toBeVisible();

    // 4. BPSec Security
    await subNav.getByRole('button', { name: /BPSec Security/i }).click();
    await expect(page.getByText('BPSec Security & Authentication')).toBeVisible();

    // 5. Storage & Bundles
    await subNav.getByRole('button', { name: /Storage & Bundles/i }).click();
    await expect(page.locator('h1')).toContainText('Bundle Storage Inspector');

    // 6. Logs & Diagnostics
    await subNav.getByRole('button', { name: /Logs & Diagnostics/i }).click();
    await expect(page.getByRole('heading', { name: /System Log Console/i })).toBeVisible();

    // 7. JSON Config
    await subNav.getByRole('button', { name: /JSON Config/i }).click();
    await expect(page.getByRole('heading', { name: 'Station JSON Configuration' })).toBeVisible();
    await expect(page.locator('pre')).toBeVisible();
  });

  test('deep-links to station sub-tabs and handles backward-compatible aliases', async ({ page }) => {
    await page.route('**/api/capabilities', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ modules: ['chat', 'files', 'senml', 'rego_routing'] }),
      });
    });
    await page.route('**/api/rego_routing/v1/blacklist', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) });
    });
    await page.route('**/api/rego_routing/v1/policy', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          config: {
            local_eid: 'dtn://f4jxq/',
            node_eids: ['dtn://f4jxq/'],
            is_digipeater: false,
            supported_aliases: [],
            trapped_aliases: [],
            hop_limit: 16,
            treb_block_type: 201,
            record_treb: true,
            active_profile: 'default',
          },
          sources: {
            constants: 'package dtn.constants',
            helpers: 'package dtn.helpers',
            ingress: 'package dtn.ingress',
            egress: 'package dtn.egress',
            routing: 'package dtn.routing',
          },
        }),
      });
    });
    await page.route('**/api/rego_routing/v1/decisions', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) });
    });
    // Route for Bundles view when embedded
    await page.route('**/api/v1/bundles*', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify([]) });
    });
    // Route for Logs view when embedded
    await page.route('**/api/v1/logs', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ logs: [], count: 0, capacity: 500 }) });
    });

    // Test direct deep link to routes
    await page.goto('/#/station/routes');
    await expect(page.getByRole('heading', { name: 'Static Routes' })).toBeVisible();

    // Test direct deep link to rego_routing sub-tab
    await page.goto('/#/station/rego_routing');
    await expect(page.getByRole('heading', { name: 'OPA / Rego Routing & Filtering' })).toBeVisible();

    // Test backward-compatible alias #/rego_routing -> Station / Rego Routing
    await page.goto('/#/rego_routing');
    await expect(page.getByRole('heading', { name: 'OPA / Rego Routing & Filtering' })).toBeVisible();

    // Test backward-compatible alias #/bundles -> Station / Bundles
    await page.goto('/#/bundles');
    await expect(page.locator('h1')).toContainText('Bundle Storage Inspector');

    // Test backward-compatible alias #/logs -> Station / Logs
    await page.goto('/#/logs');
    await expect(page.getByRole('heading', { name: /System Log Console/i })).toBeVisible();
  });
});
