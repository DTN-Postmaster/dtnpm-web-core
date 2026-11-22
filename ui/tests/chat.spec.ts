import { test, expect } from '@playwright/test';

const mockStationConfig = {
  status: 'running',
  version: '0.1.0',
  callsign: 'STATA',
  node_id: 'dtn://stata/',
  admin_endpoints: ['dtn://stata/~admin'],
  storage: { in_memory: true },
  grpc: { enabled: false },
  web: { address: '0.0.0.0:8080', static_dir: './web/dist' },
  clas: [
    {
      name: 'tcp-ax25',
      type: 'tcpclv4',
      peers: [{ callsign: 'F4JXQ', node_id: 'dtn://f4jxq/' }],
    },
  ],
  routes: [],
};

const mockConversations = [
  {
    peer: 'dtn://f4jxq/chat',
    callsign: 'F4JXQ',
    last_message: '73 de F4JXQ! All copy 59.',
    last_direction: 'inbound',
    last_status: 'received',
    last_timestamp: 1730764800000,
    last_created_at: '2026-11-04T12:00:00Z',
    unread_count: 2,
    total_messages: 5,
  },
  {
    peer: 'dtn://on7knt/chat',
    callsign: 'ON7KNT',
    last_message: 'QSL via satellite pass',
    last_direction: 'outbound',
    last_status: 'sent',
    last_timestamp: 1730760000000,
    last_created_at: '2026-11-04T10:00:00Z',
    unread_count: 0,
    total_messages: 2,
  },
];

const mockF4jxqMessages = [
  {
    id: 'bundle-001',
    conversation_peer: 'dtn://f4jxq/chat',
    sender_eid: 'dtn://stata/chat',
    recipient_eid: 'dtn://f4jxq/chat',
    content: 'Hello F4JXQ de STATA on VHF packet',
    direction: 'outbound',
    status: 'sent',
    is_read: true,
    timestamp: 1730764500000,
    created_at: '2026-11-04T11:55:00Z',
  },
  {
    id: 'bundle-002',
    conversation_peer: 'dtn://f4jxq/chat',
    sender_eid: 'dtn://f4jxq/chat',
    recipient_eid: 'dtn://stata/chat',
    content: '73 de F4JXQ! All copy 59.',
    direction: 'inbound',
    status: 'received',
    is_read: false,
    timestamp: 1730764800000,
    created_at: '2026-11-04T12:00:00Z',
  },
];

test.describe('Interactive Chat Service & Messenger', () => {
  test.beforeEach(async ({ page }) => {
    // Mock station config & services
    await page.route('**/api/v1/config', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockStationConfig),
      });
    });

    await page.route('**/api/v1/services', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify([
          { eid: 'dtn://stata/chat', type: 'Built-in Chat', registered_at: '2026-11-04T00:00:00Z' },
        ]),
      });
    });

    await page.route('**/api/v1/events', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body: 'event: connected\ndata: {}\n\n',
      });
    });

    // Mock chat endpoints
    await page.route('**/api/chat/v1/conversations', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockConversations),
      });
    });

    await page.route(/\/api\/chat\/v1\/messages\?peer=.*/, async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(mockF4jxqMessages),
      });
    });

    await page.route('**/api/chat/v1/read', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok', marked: 1 }),
      });
    });

    await page.goto('/');
  });

  test('renders Chat tab in Navbar with unread badge and switches to ChatView', async ({ page }) => {
    // Find Chat button in navbar
    const chatTabBtn = page.locator('header nav button', { hasText: 'Chat' });
    await expect(chatTabBtn).toBeVisible();

    // Verify unread badge is 2
    await expect(chatTabBtn.locator('span', { hasText: '2' })).toBeVisible();

    // Click on Chat tab
    await chatTabBtn.click();

    // Verify header title
    await expect(page.locator('h1', { hasText: 'Interactive Chat' })).toBeVisible();
    await expect(page.locator('code', { hasText: 'dtn://stata/chat' })).toBeVisible();
  });

  test('displays conversation list and thread messages', async ({ page }) => {
    await page.locator('header nav button', { hasText: 'Chat' }).click();

    // Verify left sidebar lists full EID for conversations
    await expect(page.locator('aside', { hasText: 'dtn://f4jxq/chat' })).toBeVisible();
    await expect(page.locator('aside', { hasText: 'dtn://on7knt/chat' })).toBeVisible();

    // Click on F4JXQ conversation
    await page.locator('aside button', { hasText: 'dtn://f4jxq/chat' }).click();

    // Verify active thread header and destination EID
    await expect(page.locator('h2', { hasText: 'dtn://f4jxq/chat' })).toBeVisible();
    await expect(page.locator('text=Destination EID: dtn://f4jxq/chat')).toBeVisible();

    // Verify message bubbles in thread section
    await expect(page.locator('section').locator('text=Hello F4JXQ de STATA on VHF packet')).toBeVisible();
    await expect(page.locator('section').locator('text=73 de F4JXQ! All copy 59.')).toBeVisible();
  });

  test('dispatches a new chat message and updates thread', async ({ page }) => {
    let dispatchedPayload: any = null;

    await page.route('**/api/chat/v1/messages', async (route) => {
      if (route.request().method() === 'POST') {
        dispatchedPayload = route.request().postDataJSON();
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            id: 'bundle-new-003',
            conversation_peer: 'dtn://f4jxq/chat',
            sender_eid: 'dtn://stata/chat',
            recipient_eid: 'dtn://f4jxq/chat',
            content: dispatchedPayload.content,
            direction: 'outbound',
            status: 'sent',
            is_read: true,
            timestamp: 1730765000000,
            created_at: '2026-11-04T12:03:00Z',
          }),
        });
      }
    });

    await page.locator('header nav button', { hasText: 'Chat' }).click();
    await page.locator('aside button', { hasText: 'dtn://f4jxq/chat' }).click();

    // Verify lifetime input field defaults to 3600s (1h)
    const lifetimeInput = page.locator('input[title="Bundle lifetime in seconds"]');
    await expect(lifetimeInput).toBeVisible();
    await expect(lifetimeInput).toHaveValue('3600');

    // Verify 15m and 30m presets were removed
    await expect(page.locator('button', { hasText: '15m' })).not.toBeVisible();
    await expect(page.locator('button', { hasText: '30m' })).not.toBeVisible();

    // Test clicking preset '72h' updates value to 259200
    await page.locator('button', { hasText: '72h' }).click();
    await expect(lifetimeInput).toHaveValue('259200');

    // Test clicking preset '1w' updates value to 604800
    await page.locator('button', { hasText: '1w' }).click();
    await expect(lifetimeInput).toHaveValue('604800');

    // Switch back to '1h' preset (3600s)
    await page.locator('button', { hasText: '1h' }).click();
    await expect(lifetimeInput).toHaveValue('3600');

    // Type message in textarea
    const input = page.locator('textarea[placeholder*="dtn://f4jxq/chat"]');
    await input.fill('Roger 59! Testing **markdown** and emojis 🚀');

    // Click Send
    await page.locator('button[type="submit"]', { hasText: 'Send' }).click();

    // Verify request payload
    expect(dispatchedPayload).not.toBeNull();
    expect(dispatchedPayload.peer).toBe('dtn://f4jxq/chat');
    expect(dispatchedPayload.content).toBe('Roger 59! Testing **markdown** and emojis 🚀');
    expect(dispatchedPayload.lifetime).toBe(3600);

    // Verify newly sent message bubble is rendered in thread with bold formatting
    await expect(page.locator('strong', { hasText: 'markdown' })).toBeVisible();
    await expect(page.locator('text=Roger 59! Testing')).toBeVisible();
  });

  test('+ New Chat modal opens, autocompletes suggestions, and creates conversation', async ({ page }) => {
    let createdPeerPayload: any = null;
    await page.route('**/api/chat/v1/conversations', async (route) => {
      if (route.request().method() === 'POST') {
        createdPeerPayload = route.request().postDataJSON();
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            peer: createdPeerPayload.peer,
            callsign: 'F4JXQ',
            last_message: null,
            last_direction: null,
            last_status: null,
            last_timestamp: null,
            last_created_at: null,
            unread_count: 0,
            total_messages: 0,
          }),
        });
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockConversations),
        });
      }
    });

    await page.locator('header nav button', { hasText: 'Chat' }).click();

    // Click + New Chat button
    await page.locator('button', { hasText: '+ New Chat' }).click();

    // Modal should be visible
    await expect(page.locator('h3', { hasText: 'Start New Chat' })).toBeVisible();

    // Click suggestion chip for F4JXQ inside modal
    const chip = page.getByRole('button', { name: 'F4JXQ', exact: true });
    await expect(chip).toBeVisible();
    await chip.click();

    // Input should be populated
    const input = page.locator('input[placeholder*="F4JXQ"]');
    await expect(input).toHaveValue('F4JXQ');

    // Submit modal
    await page.locator('button', { hasText: 'Open Chat' }).click();

    // Modal closes
    await expect(page.locator('h3', { hasText: 'Start New Chat' })).not.toBeVisible();

    // Verify conversation was created via POST /api/chat/v1/conversations with normalized target
    expect(createdPeerPayload).not.toBeNull();
    expect(createdPeerPayload.peer).toBe('dtn://f4jxq/chat');

    // Thread is opened with dtn://f4jxq/chat
    await expect(page.locator('h2', { hasText: 'dtn://f4jxq/chat' })).toBeVisible();
    await expect(page.locator('text=Destination EID: dtn://f4jxq/chat')).toBeVisible();
  });

  test('handles explicit service and SSID like dtn://f4jxq-9/chat without duplicate service or callsign stripping', async ({ page }) => {
    let createdPeerPayload: any = null;
    await page.route('**/api/chat/v1/conversations', async (route) => {
      if (route.request().method() === 'POST') {
        createdPeerPayload = route.request().postDataJSON();
        await route.fulfill({
          status: 201,
          contentType: 'application/json',
          body: JSON.stringify({
            peer: createdPeerPayload.peer,
            callsign: 'F4JXQ-9',
            last_message: null,
            last_direction: null,
            last_status: null,
            last_timestamp: null,
            last_created_at: null,
            unread_count: 0,
            total_messages: 0,
          }),
        });
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(mockConversations),
        });
      }
    });

    await page.locator('header nav button', { hasText: 'Chat' }).click();
    await page.locator('button', { hasText: '+ New Chat' }).click();

    const input = page.locator('input[placeholder*="F4JXQ"]');
    await input.fill('dtn://f4jxq-9/chat');

    // Real-time helper text preview must show exact normalized target without redundant /chat
    await expect(page.locator('span', { hasText: 'dtn://f4jxq-9/chat' })).toBeVisible();

    await page.locator('button', { hasText: 'Open Chat' }).click();

    expect(createdPeerPayload).not.toBeNull();
    expect(createdPeerPayload.peer).toBe('dtn://f4jxq-9/chat');

    // Verify header and destination EID do not contain /chat/chat or strip SSID
    await expect(page.locator('h2', { hasText: 'dtn://f4jxq-9/chat' })).toBeVisible();
    await expect(page.locator('text=Destination EID: dtn://f4jxq-9/chat')).toBeVisible();
    await expect(page.getByText('/chat/chat')).not.toBeVisible();
  });

  test('newly sent message does not disappear or re-scroll when a stale background poll resolves after POST', async ({ page }) => {
    await page.clock.install();

    let pollCount = 0;
    let releaseStalePoll: (() => void) | null = null;
    let currentMessages = [...mockF4jxqMessages];

    await page.route('**/api/chat/v1/messages**', async (route) => {
      if (route.request().method() === 'GET') {
        pollCount += 1;
        if (pollCount === 2) {
          // Hold the 2nd GET (first 4s background poll) pending until after POST completes
          await new Promise<void>((resolve) => {
            releaseStalePoll = resolve;
          });
          await route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify(mockF4jxqMessages),
          });
          return;
        }
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(currentMessages),
        });
      } else if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON();
        const sentMsg = {
          id: 'bundle-smooth-001',
          conversation_peer: body.peer,
          sender_eid: 'dtn://stata/chat',
          recipient_eid: body.peer,
          content: body.content,
          direction: 'outbound',
          status: 'sent',
          is_read: true,
          timestamp: Date.now(),
          created_at: new Date().toISOString(),
        };
        currentMessages = [...currentMessages, sentMsg];
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(sentMsg),
        });
      }
    });

    await page.locator('header nav button', { hasText: 'Chat' }).click();
    await expect(page.locator('text=Hello F4JXQ de STATA on VHF packet')).toBeVisible();

    // Advance 4s so the background poll (#2) starts and stays in-flight
    await page.clock.runFor(4100);
    await expect.poll(() => releaseStalePoll).not.toBeNull();

    // Send a new message while poll #2 is in-flight
    const textarea = page.locator('textarea[placeholder*="dtn://f4jxq/chat"]');
    await textarea.fill('Smooth message without flicker');
    await page.locator('button[type="submit"]', { hasText: 'Send' }).click();
    await expect(page.locator('text=Smooth message without flicker')).toBeVisible();

    // Track any DOM removal of the sent message node
    await page.evaluate(() => {
      (window as any).__removedSentNode = false;
      const container = document.querySelector('div.overflow-y-auto.space-y-3');
      if (!container) return;
      const observer = new MutationObserver((mutations) => {
        for (const m of mutations) {
          for (const node of m.removedNodes) {
            if (node instanceof HTMLElement && node.innerText.includes('Smooth message without flicker')) {
              (window as any).__removedSentNode = true;
            }
          }
        }
      });
      observer.observe(container, { childList: true, subtree: true });
    });

    // Resolve the stale pre-send poll #2 and wait for browser to process its response
    const stalePollResponse = page.waitForResponse(
      (res) => res.url().includes('/api/chat/v1/messages') && res.request().method() === 'GET'
    );
    releaseStalePoll!();
    await stalePollResponse;
    await page.clock.runFor(100);

    // Message must remain continuously visible and never have been removed from DOM
    await expect(page.locator('text=Smooth message without flicker')).toBeVisible();
    const wasRemoved = await page.evaluate(() => (window as any).__removedSentNode);
    expect(wasRemoved).toBe(false);

    // Subsequent poll (#3) also keeps it visible without removal
    const nextPollResponse = page.waitForResponse(
      (res) => res.url().includes('/api/chat/v1/messages') && res.request().method() === 'GET'
    );
    await page.clock.runFor(4100);
    await nextPollResponse;
    await expect(page.locator('text=Smooth message without flicker')).toBeVisible();
    const wasRemovedAfterNextPoll = await page.evaluate(() => (window as any).__removedSentNode);
    expect(wasRemovedAfterNextPoll).toBe(false);
  });

  test('background polling does not hijack scroll when user scrolled up, while sending scrolls to bottom', async ({ page }) => {
    await page.clock.install();

    const manyMessages = Array.from({ length: 25 }, (_, idx) => ({
      id: `bundle-hist-${idx}`,
      conversation_peer: 'dtn://f4jxq/chat',
      sender_eid: idx % 2 === 0 ? 'dtn://stata/chat' : 'dtn://f4jxq/chat',
      recipient_eid: idx % 2 === 0 ? 'dtn://f4jxq/chat' : 'dtn://stata/chat',
      content: `History message number ${idx + 1}`,
      direction: idx % 2 === 0 ? 'outbound' : 'inbound',
      status: idx === 24 ? 'queued' : 'sent',
      is_read: true,
      timestamp: 1730764500000 + idx * 1000,
      created_at: new Date(1730764500000 + idx * 1000).toISOString(),
    }));

    let pollNum = 0;
    await page.route('**/api/chat/v1/messages**', async (route) => {
      if (route.request().method() === 'GET') {
        pollNum += 1;
        // On 2nd poll, simulate status transition queued -> sent on last message
        const payload =
          pollNum >= 2
            ? manyMessages.map((m, i) => (i === 24 ? { ...m, status: 'sent' } : m))
            : manyMessages;
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(payload),
        });
      } else if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON();
        const sentMsg = {
          id: 'bundle-new-bottom',
          conversation_peer: body.peer,
          sender_eid: 'dtn://stata/chat',
          recipient_eid: body.peer,
          content: body.content,
          direction: 'outbound',
          status: 'sent',
          is_read: true,
          timestamp: Date.now(),
          created_at: new Date().toISOString(),
        };
        manyMessages.push(sentMsg);
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(sentMsg),
        });
      }
    });

    await page.locator('header nav button', { hasText: 'Chat' }).click();
    await expect(page.locator('text=History message number 25')).toBeVisible();

    // Wait for initial auto-scroll to settle at bottom before installing spy & scrolling up
    await expect
      .poll(async () =>
        page.evaluate(() => {
          const el = document.querySelector('div.overflow-y-auto.space-y-3') as HTMLElement;
          return el ? el.scrollHeight - el.scrollTop - el.clientHeight : 999;
        })
      )
      .toBeLessThanOrEqual(20);

    // Spy on scrollIntoView calls and scroll up to the very top of the thread
    const initialScrollTop = await page.evaluate(() => {
      (window as any).__scrollIntoViewCalls = 0;
      const orig = Element.prototype.scrollIntoView;
      Element.prototype.scrollIntoView = function (...args: any[]) {
        (window as any).__scrollIntoViewCalls += 1;
        return orig.apply(this, args as any);
      };
      const el = document.querySelector('div.overflow-y-auto.space-y-3') as HTMLElement;
      el.scrollTop = 0;
      return el.scrollTop;
    });
    expect(initialScrollTop).toBe(0);

    // Trigger 4s background poll (status update queued -> sent, no new messages) and wait for response
    const pollResp = page.waitForResponse(
      (res) => res.url().includes('/api/chat/v1/messages') && res.request().method() === 'GET'
    );
    await page.clock.runFor(4100);
    await pollResp;
    await page.clock.runFor(100);

    // Background poll must NOT call scrollIntoView or snap scrollTop away from top
    const scrollCallsAfterPoll = await page.evaluate(() => (window as any).__scrollIntoViewCalls);
    expect(scrollCallsAfterPoll).toBe(0);
    const scrollAfterPoll = await page.evaluate(() => {
      const el = document.querySelector('div.overflow-y-auto.space-y-3') as HTMLElement;
      return el.scrollTop;
    });
    expect(scrollAfterPoll).toBe(0);

    // Now send a new message — should scroll to bottom to show the sent message
    const textarea = page.locator('textarea[placeholder*="dtn://f4jxq/chat"]');
    await textarea.fill('Back to bottom on send');
    await page.locator('button[type="submit"]', { hasText: 'Send' }).click();
    await expect(page.locator('text=Back to bottom on send')).toBeVisible();

    await expect
      .poll(async () =>
        page.evaluate(() => {
          const el = document.querySelector('div.overflow-y-auto.space-y-3') as HTMLElement;
          return el.scrollHeight - el.scrollTop - el.clientHeight;
        })
      )
      .toBeLessThanOrEqual(20);
  });
});
