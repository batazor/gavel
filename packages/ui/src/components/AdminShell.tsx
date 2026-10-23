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
  /** One sentence about the data: the finding, the period, the scope. Never about the interface. */
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
  The frame: a dark ink sidebar with the copper mark, sticky beside a parchment canvas that scrolls with
  the document, so the wheel works wherever the cursor is. No top bar on desktop: the title row sits on
  the canvas. Below lg the sidebar folds into a 48px ink bar and a side drawer.
*/
export function AdminShell({ title, subtitle, nav, actions, children, className, searchItems = [], scope = 'Operations · Berlin desk', user = { name: 'Elena Volkova', role: 'Senior operator', initials: 'EV' } }: AdminShellProps) {
  const [searchOpen, setSearchOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const allSearchItems = [...searchItems, ...nav.map((item) => ({ label: item.label, meta: item.badge ? `${item.badge} waiting` : '', group: 'Go to', onSelect: item.onClick }))];

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); setSearchOpen(true); }
      if (event.key === 'Escape') setMenuOpen(false);
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
              'flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-sm transition-[background-color,color,transform] duration-150',
              item.active ? 'bg-surface text-ink' : 'text-sidebar-muted hover:bg-sidebar-hover hover:text-sidebar-ink',
            )}
          >
            {item.icon && <span aria-hidden="true" className="grid size-4 shrink-0 place-items-center [&>svg]:size-4 [&>svg]:stroke-[1.5]">{item.icon}</span>}
            <span className="flex-1 truncate">{item.label}</span>
            {item.badge && <span className={cn('rounded-md px-1.5 text-xs font-semibold tabular-nums', item.active ? 'bg-copper-pale text-copper-deep' : 'bg-copper text-surface')}>{item.badge}</span>}
          </button>
        </li>
      ))}
    </ul>
  );

  const brand = (
    <div className="flex items-center gap-3 px-1">
      <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-copper text-surface"><GavelMark size={20} /></span>
      <div className="min-w-0">
        <p className="text-sm font-semibold tracking-[0.08em] text-sidebar-ink">GAVEL</p>
        <p className="truncate text-xs text-sidebar-muted">{scope}</p>
      </div>
    </div>
  );

  const userRow = (
    <div className="flex items-center gap-2.5 px-1">
      <span aria-hidden="true" className="grid size-8 shrink-0 place-items-center rounded-full bg-copper-pale text-xs font-semibold text-copper-deep">{user.initials}</span>
      <div className="min-w-0">
        <p className="truncate text-sm text-sidebar-ink">{user.name}</p>
        <p className="truncate text-xs text-sidebar-muted">{user.role}</p>
      </div>
    </div>
  );

  return (
    <div className={cn('flex min-h-dvh w-full bg-ground', className)}>
      <aside className="sticky top-0 hidden h-dvh w-60 shrink-0 flex-col overflow-y-auto bg-sidebar px-3 py-5 lg:flex">
        {brand}
        <button type="button" onClick={() => setSearchOpen(true)} className="mt-5 flex h-9 items-center gap-2 rounded-lg border border-sidebar-line bg-sidebar-hover px-2.5 text-sm text-sidebar-muted transition-colors hover:border-sidebar-muted/40 hover:text-sidebar-ink">
          <Search size={14} strokeWidth={1.5} />
          <span className="flex-1 text-left">Search</span>
          <span className="text-xs">⌘K</span>
        </button>
        <nav aria-label="Sections" className="mt-6">{navList()}</nav>
        <div className="mt-auto border-t border-sidebar-line pt-4">{userRow}</div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-30 flex h-12 shrink-0 items-center gap-1 bg-sidebar px-3 text-sidebar-ink lg:hidden">
          <button type="button" aria-label="Open menu" aria-expanded={menuOpen} onClick={() => setMenuOpen(true)} className="grid size-10 place-items-center rounded-lg hover:bg-sidebar-hover"><Menu size={20} strokeWidth={1.5} /></button>
          <span className="grid size-7 place-items-center rounded-md bg-copper text-surface"><GavelMark size={16} /></span>
          <span className="ml-1 min-w-0 flex-1 truncate text-sm font-medium">{title}</span>
          <button type="button" aria-label="Search" onClick={() => setSearchOpen(true)} className="grid size-10 place-items-center rounded-lg hover:bg-sidebar-hover"><Search size={18} strokeWidth={1.5} /></button>
        </header>

        <main className="min-w-0 flex-1">
          <div className="mx-auto w-full max-w-[1400px] px-5 py-5 sm:px-6 sm:py-6 xl:px-8 xl:py-7">
            <div className="mb-5 flex flex-wrap items-end justify-between gap-x-6 gap-y-3 sm:mb-6">
              <div className="min-w-0">
                <h1 className="text-2xl font-semibold tracking-[-0.02em] text-ink">{title}</h1>
                {subtitle && <p className="mt-1 text-sm text-muted">{subtitle}</p>}
              </div>
              {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
            </div>
            {children}
          </div>
        </main>
      </div>

      {menuOpen && (
        <div className="fixed inset-0 z-40 lg:hidden" role="dialog" aria-modal="true" aria-label="Menu">
          <button type="button" aria-label="Close menu" onClick={() => setMenuOpen(false)} className="absolute inset-0 bg-ink/40" />
          <div className="absolute inset-y-0 left-0 flex w-[280px] max-w-[85vw] flex-col bg-sidebar px-3 py-5 shadow-float">
            <div className="flex items-center justify-between">
              {brand}
              <button type="button" aria-label="Close menu" onClick={() => setMenuOpen(false)} className="grid size-10 place-items-center rounded-lg text-sidebar-ink hover:bg-sidebar-hover"><X size={20} strokeWidth={1.5} /></button>
            </div>
            <nav aria-label="Sections" className="mt-6">{navList(() => setMenuOpen(false))}</nav>
            <div className="mt-auto border-t border-sidebar-line pt-4">{userRow}</div>
          </div>
        </div>
      )}

      {searchOpen && <AdminSearch items={allSearchItems} onClose={() => setSearchOpen(false)} />}
    </div>
  );
}
