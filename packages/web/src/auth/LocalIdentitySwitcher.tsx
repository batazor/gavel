import { useState } from 'react';
import {
  clearLocalIdentity,
  currentLocalActor,
  localActors,
  localAuthConfiguration,
  selectLocalIdentity,
  type LocalActorId,
} from './localIdentity';

export function LocalIdentitySwitcher() {
  const config = localAuthConfiguration();
  const [actorId, setActorId] = useState<LocalActorId | ''>(() => currentLocalActor()?.id ?? '');
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!config.enabled) return null;

  const changeIdentity = async (next: string) => {
    setError(null);
    if (!next) {
      clearLocalIdentity();
      setActorId('');
      window.location.reload();
      return;
    }
    setPending(true);
    try {
      const actor = await selectLocalIdentity(next as LocalActorId);
      setActorId(actor.id);
      window.location.reload();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Unable to select local identity.');
      setPending(false);
    }
  };

  return (
    <aside className="fixed bottom-3 right-3 z-50 w-64 border border-rule bg-raised p-3 shadow-lg" aria-label="Local development identity">
      <label className="block text-xs font-medium uppercase tracking-wide text-muted" htmlFor="local-identity">Local identity</label>
      <select
        id="local-identity"
        className="mt-2 h-9 w-full border border-rule bg-ground px-2 text-sm text-ink"
        value={actorId}
        disabled={pending}
        onChange={(event) => void changeIdentity(event.target.value)}
      >
        <option value="">Unauthenticated</option>
        {localActors.map((actor) => <option key={actor.id} value={actor.id}>{actor.label} · {actor.subject}</option>)}
      </select>
      {error && <p className="mt-2 text-xs text-negative" role="alert">{error}</p>}
      <p className="mt-2 text-xs text-muted">Development only · short-lived JWT</p>
    </aside>
  );
}
