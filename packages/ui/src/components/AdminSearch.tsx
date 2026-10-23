import { useEffect, useMemo, useState } from 'react';
import { Search } from 'lucide-react';

export interface AdminSearchItem {
  label: string;
  meta: string;
  group: string;
  onSelect?: () => void;
}

/* A floating dialog: the one surface that carries a shadow. Rows on a 44px pitch, the group in grey at the right. */
export function AdminSearch({ items, onClose }: { items: AdminSearchItem[]; onClose: () => void }) {
  const [query, setQuery] = useState('');
  const results = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return items;
    return items.filter((item) => `${item.label} ${item.meta} ${item.group}`.toLowerCase().includes(normalized));
  }, [items, query]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-ink/30 p-4 pt-[12vh]" onClick={onClose}>
      <section role="dialog" aria-modal="true" aria-label="Search" onClick={(event) => event.stopPropagation()} className="w-full max-w-xl overflow-hidden rounded-[8px] border border-line bg-surface shadow-float">
        <div className="flex h-12 items-center gap-3 border-b border-line px-4">
          <Search size={16} className="shrink-0 text-muted" strokeWidth={1.5} />
          <input
            autoFocus
            aria-label="Search lots, customers and settlements"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => { if (event.key === 'Enter' && results[0]) { results[0].onSelect?.(); onClose(); } }}
            placeholder="Lot, customer, case or workflow"
            className="min-w-0 flex-1 border-0 bg-transparent text-sm outline-none placeholder:text-faint"
          />
          <span className="text-xs text-faint">Esc</span>
        </div>
        <div className="max-h-[52vh] overflow-y-auto py-1">
          {results.length ? (
            <ul>
              {results.map((item) => (
                <li key={`${item.group}-${item.label}`}>
                  <button type="button" onClick={() => { item.onSelect?.(); onClose(); }} className="flex h-11 w-full items-center gap-3 px-4 text-left hover:bg-raised focus-visible:bg-raised focus-visible:outline-none">
                    <span className="min-w-0 flex-1 truncate text-sm text-ink">{item.label}<span className="ml-2 text-xs text-muted">{item.meta}</span></span>
                    <span className="shrink-0 text-xs text-faint">{item.group}</span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="px-4 py-8 text-center text-sm text-muted">Nothing matches "{query}". Try a lot ID, a customer name or a case number.</p>
          )}
        </div>
        <footer className="flex h-9 items-center justify-between border-t border-line px-4 text-xs text-faint">
          <span>{results.length} of {items.length}</span>
          <span>Enter opens the first result</span>
        </footer>
      </section>
    </div>
  );
}
