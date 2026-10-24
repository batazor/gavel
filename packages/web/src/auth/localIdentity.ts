export type LocalActorId = 'seller' | 'bidder-one' | 'bidder-two';

export type LocalActor = {
  id: LocalActorId;
  label: string;
  subject: string;
  clientId: string;
};

export const localActors: readonly LocalActor[] = [
  { id: 'seller', label: 'Seller', subject: 'seller-local', clientId: 'gavel-local-seller' },
  { id: 'bidder-one', label: 'Bidder 1', subject: 'bidder-one-local', clientId: 'gavel-local-bidder-one' },
  { id: 'bidder-two', label: 'Bidder 2', subject: 'bidder-two-local', clientId: 'gavel-local-bidder-two' },
] as const;

const storageKey = 'gavel.local-identity.v1';

type StoredIdentity = {
  actorId: LocalActorId;
  accessToken: string;
  expiresAt: number;
};

type TokenResponse = {
  access_token?: string;
  expires_in?: number;
};

type JwtPayload = {
  iss?: string;
  sub?: string;
  aud?: string | string[];
  exp?: number;
};

export function localAuthConfiguration() {
  const enabled = import.meta.env.VITE_LOCAL_AUTH_ENABLED === 'true';
  const profile = import.meta.env.VITE_DEPLOYMENT_PROFILE?.trim() || 'production';
  const issuer = import.meta.env.VITE_LOCAL_AUTH_ISSUER?.trim() || '';
  const audience = import.meta.env.VITE_LOCAL_AUTH_AUDIENCE?.trim() || '';

  if (enabled && profile !== 'local') {
    throw new Error('VITE_LOCAL_AUTH_ENABLED is permitted only with VITE_DEPLOYMENT_PROFILE=local.');
  }
  if (enabled && (!issuer || !audience)) {
    throw new Error('Local auth requires VITE_LOCAL_AUTH_ISSUER and VITE_LOCAL_AUTH_AUDIENCE.');
  }
  return { enabled, profile, issuer, audience };
}

function decodePayload(token: string): JwtPayload {
  const encoded = token.split('.')[1];
  if (!encoded) throw new Error('The local issuer returned a malformed access token.');
  const normalized = encoded.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
  return JSON.parse(atob(padded)) as JwtPayload;
}

function audienceContains(payload: JwtPayload, audience: string) {
  return Array.isArray(payload.aud) ? payload.aud.includes(audience) : payload.aud === audience;
}

function readStoredIdentity(): StoredIdentity | null {
  const raw = window.sessionStorage.getItem(storageKey);
  if (!raw) return null;
  try {
    const stored = JSON.parse(raw) as StoredIdentity;
    if (!localActors.some(({ id }) => id === stored.actorId) || stored.expiresAt <= Date.now() + 5_000) {
      window.sessionStorage.removeItem(storageKey);
      return null;
    }
    return stored;
  } catch {
    window.sessionStorage.removeItem(storageKey);
    return null;
  }
}

export function currentLocalActor() {
  const actorId = readStoredIdentity()?.actorId;
  return localActors.find(({ id }) => id === actorId) ?? null;
}

export function localAccessToken() {
  return readStoredIdentity()?.accessToken ?? null;
}

export function clearLocalIdentity() {
  window.sessionStorage.removeItem(storageKey);
}

export async function selectLocalIdentity(actorId: LocalActorId) {
  const config = localAuthConfiguration();
  if (!config.enabled) throw new Error('The development identity switcher is disabled.');
  const actor = localActors.find(({ id }) => id === actorId);
  if (!actor) throw new Error('Unknown local actor.');

  const body = new URLSearchParams({
    grant_type: 'client_credentials',
    client_id: actor.clientId,
    client_secret: 'local-development-only',
  });
  const response = await fetch(`${config.issuer}/token`, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body,
  });
  if (!response.ok) throw new Error(`Local issuer returned HTTP ${response.status}.`);
  const result = await response.json() as TokenResponse;
  if (!result.access_token) throw new Error('Local issuer returned no access token.');

  const payload = decodePayload(result.access_token);
  if (payload.iss !== config.issuer || payload.sub !== actor.subject || !audienceContains(payload, config.audience)) {
    throw new Error('Local issuer returned claims that do not match the selected actor.');
  }
  const expiresAt = (payload.exp ?? Math.floor(Date.now() / 1000) + (result.expires_in ?? 300)) * 1_000;
  window.sessionStorage.setItem(storageKey, JSON.stringify({ actorId, accessToken: result.access_token, expiresAt } satisfies StoredIdentity));
  return actor;
}
