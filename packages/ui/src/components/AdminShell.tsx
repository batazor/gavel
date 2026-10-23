import { useEffect, useState, type ReactNode } from 'react';
import { Menu, Search, X } from 'lucide-react';
import { cn } from '../lib/cn';
import { AdminSearch, type AdminSearchItem } from './AdminSearch';
import { GavelMark } from './GavelMark';

export interface AdminNavItem {
  label: string;
  icon?: ReactNode;
  active?: boolean;
  /** A count only on a queue that needs action, with its noun clear from the label. */
  badge?: string;
  onClick?: () => void;
}

export interface AdminShellProps {
  /** The screen's name: a noun, set as the one h1. */
  title: string;
  /** One grey sentence about the data: the finding, the period, the scope. Never about the interface. */
  subtitle?: ReactNode;
  nav: AdminNavItem[];
  /** Real actions only; the one primary of the screen lives here. */
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  searchItems?: AdminSearchItem[];
  scope?: string;
  user?: { name: string; role: string; initials: string };
}

/*
  Open plane: a 240px labelled sidebar on the page ground, one hairline to the canvas, no top bar.
  The product's mark and name at the top, the desk's scope under it, search as a row, the nav on a 34px pitch
  with a neutral grey pill on the active item, utilities and the operator at the bottom.
  On a phone the sidebar folds into a 48px bar with the screen's name and a menu that opens a full-screen sheet.
*/
export function AdminShell({ title, subtitle, nav, actions, children, className, searchItems = [], scope = 'Operations · Berlin desk', user = { name: 'Elena Volkova', role: 'Senior operator', initials: 'EV' } }: AdminShellProps) {
  const [searchOpen, setSearchOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const allSearchItems = [...searchItems, ...nav.map((item) => ({ label: item.label, meta: item.badge ? `${item.badge} waiting` : '', group: 'Go to', onSelect: item.onClick }))];

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); setSearchOpen(true); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const navList = (onNavigate?: () => void) => (
    <ul className="flex flex-col gap-0.5">
      {nav.map((item) => (
        <li key={item.label}>
          <button
            type="button"
            onClick={() => { item.onClick?.(); onNavigate?.(); }}
            aria-current={item.active ? 'page' : undefined}
            className={cn(
              'flex h-8 w-full items-center gap-2.5 rounded-[6px] px-2 text-left text-sm transition-colors duration-150 lg:h-8 max-lg:h-11',
              item.active ? 'bg-active text-ink' : 'text-muted hover:bg-raised hover:text-ink',
            )}
          >
            {item.icon && <span aria-hidden="true" className="grid size-4 shrink-0 place-items-center [&>svg]:size-4 [&>svg]:stroke-[1.5]">{item.icon}</span>}
            <span className="flex-1 truncate">{item.label}</span>
            {item.badge && <span className="text-xs font-medium text-ink tabular-nums">{item.badge}</span>}
          </button>
        </li>
      ))}
    </ul>
  );

  const brand = (
    <div className="flex items-center gap-2.5 px-2">
      <GavelMark size={22} className="text-copper" />
      <div className="min-w-0">
        <p className="text-sm font-semibold text-ink">Gavel</p>
        <p className="truncate text-xs text-muted">{scope}</p>
      </div>
    </div>
  );

  return (
    <div className={cn('flex h-full min-h-0 w-full bg-ground', className)}>
      <aside className="hidden w-60 shrink-0 flex-col border-r border-line px-3 py-4 lg:flex">
        {brand}
        <button type="button" onClick={() => setSearchOpen(true)} className="mt-4 flex h-8 items-center gap-2 rounded-[6px] border border-control bg-surface px-2.5 text-sm text-muted hover:bg-raised">
          <Search size={14} strokeWidth={1.5} />
          <span className="flex-1 text-left">Search</span>
          <span className="text-xs text-faint">⌘K</span>
        </button>
        <nav aria-label="Sections" className="mt-5">{navList()}</nav>
        <div className="mt-auto flex items-center gap-2.5 px-2 pt-4">
          <span aria-hidden="true" className="grid size-7 place-items-center rounded-full bg-copper-pale text-xs font-medium text-copper-deep">{user.initials}</span>
          <div className="min-w-0">
            <p className="truncate text-sm text-ink">{user.name}</p>
            <p className="truncate text-xs text-muted">{user.role}</p>
          </div>
        </div>
      </aside>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b border-line bg-ground px-4 lg:hidden">
          <button type="button" aria-label="Open menu" aria-expanded={menuOpen} onClick={() => setMenuOpen(true)} className="grid size-11 place-items-center rounded-[6px] text-ink hover:bg-raised"><Menu size={20} strokeWidth={1.5} /></button>
          <GavelMark size={20} className="text-copper" />
          <span className="min-w-0 flex-1 truncate text-sm font-medium text-ink">{title}</span>
          <button type="button" aria-label="Search" onClick={() => setSearchOpen(true)} className="grid size-11 place-items-center rounded-[6px] text-ink hover:bg-raised"><Search size={18} strokeWidth={1.5} /></button>
        </header>

        <main className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
          <div className="mx-auto w-full max-w-[1400px] px-5 py-5 sm:px-6 sm:py-6 xl:px-8 xl:py-7">
            <div className="mb-5 flex flex-wrap items-end justify-between gap-x-6 gap-y-3 sm:mb-6">
              <div className="min-w-0">
                <h1 className="text-2xl font-semibold tracking-[-0.01em] text-ink">{title}</h1>
                {subtitle && <p className="mt-1 text-sm text-muted">{subtitle}</p>}
              </div>
              {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
            </div>
            {children}
          </div>
        </main>
      </div>

      {menuOpen && (
        <div className="fixed inset-0 z-40 flex flex-col bg-surface px-3 py-4 lg:hidden" role="dialog" aria-modal="true" aria-label="Menu">
          <div className="flex items-center justify-between">
            {brand}
            <button type="button" aria-label="Close menu" onClick={() => setMenuOpen(false)} className="grid size-11 place-items-center rounded-[6px] text-ink hover:bg-raised"><X size={20} strokeWidth={1.5} /></button>
          </div>
          <nav aria-label="Sections" className="mt-6">{navList(() => setMenuOpen(false))}</nav>
          <div className="mt-auto flex items-center gap-2.5 px-2">
            <span aria-hidden="true" className="grid size-8 place-items-center rounded-full bg-copper-pale text-xs font-medium text-copper-deep">{user.initials}</span>
            <div><p className="text-sm text-ink">{user.name}</p><p className="text-xs text-muted">{user.role}</p></div>
          </div>
        </div>
      )}

      {searchOpen && <AdminSearch items={allSearchItems} onClose={() => setSearchOpen(false)} />}
    </div>
  );
}
