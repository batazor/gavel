import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

type SeedState = {
  lotId: string;
  auctionId: string;
};

type MailpitMessage = {
  ID: string;
  Subject: string;
  Snippet: string;
};

type MailpitMessages = {
  messages: MailpitMessage[];
};

const statePath = process.env.GAVEL_E2E_STATE_FILE;
if (!statePath) throw new Error('GAVEL_E2E_STATE_FILE is required. Run make mvp-e2e.');
const fixture = JSON.parse(readFileSync(statePath, 'utf8')) as SeedState;
const mailpitUrl = process.env.GAVEL_E2E_MAILPIT_URL ?? 'http://127.0.0.1:8025';

async function selectIdentity(page: Page, actor: 'bidder-one' | 'bidder-two') {
  await page.getByLabel('Local identity').selectOption(actor);
  await expect(page.getByLabel('Local identity')).toHaveValue(actor);
  await expect(page.getByText('Live data')).toBeVisible();
}

async function placeMaximum(page: Page, maximum: number) {
  const input = page.getByLabel(/Your maximum/);
  await expect(input).toBeEnabled({ timeout: 15_000 });
  await input.fill(String(maximum));
  await page.getByRole('button', { name: 'Place bid' }).click();
  await expect(page.getByRole('status')).toContainText('accepted');
}

function temporalWorkflowStatus(workflowId: string) {
  try {
    const output = execFileSync(
      'kubectl',
      [
        '--context', 'kind-gavel-mvp', '-n', 'gavel-mvp',
        'exec', 'deployment/temporal', '--',
        'temporal', 'workflow', 'describe',
        '--address', 'temporal-frontend:7233',
        '--namespace', 'gavel',
        '--workflow-id', workflowId,
        '--output', 'json',
      ],
      { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] },
    );
    const description = JSON.parse(output) as { workflowExecutionInfo?: { status?: string } };
    return description.workflowExecutionInfo?.status ?? '';
  } catch {
    return '';
  }
}

async function winnerMail(request: APIRequestContext) {
  const response = await request.get(`${mailpitUrl}/api/v1/messages`);
  if (!response.ok()) return null;
  const inbox = await response.json() as MailpitMessages;
  return inbox.messages.find((message) =>
    message.Subject === 'Покупка подтверждена' && message.Snippet.includes(fixture.auctionId),
  ) ?? null;
}

test('two bidders settle one auction and notify the winner', async ({ page, request }) => {
  await page.goto(`/admin/lots/${encodeURIComponent(fixture.lotId)}`);

  await selectIdentity(page, 'bidder-one');
  await placeMaximum(page, 15_000);

  await selectIdentity(page, 'bidder-two');
  await placeMaximum(page, 20_000);
  await expect(page.getByText('You are leading')).toBeVisible();

  await expect(page.getByText('Sold to you. The result is final.')).toBeVisible();
  await expect.poll(
    () => temporalWorkflowStatus(`settle-auction/${fixture.auctionId}`),
    { message: 'settlement workflow should complete exactly once' },
  ).toMatch(/COMPLETED/i);
  await expect.poll(
    () => winnerMail(request),
    { message: 'winner notification should reach Mailpit' },
  ).not.toBeNull();
});
