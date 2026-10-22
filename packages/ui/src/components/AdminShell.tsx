import type { ReactNode } from 'react';
import { Gavel, Search } from 'lucide-react';
import { cn } from '../lib/cn';

export interface AdminNavItem {
  label: string;
  icon?: ReactNode;
  active?: boolean;
  badge?: string;
  onClick?: () => void;
}

export interface AdminShellProps {
  title: string;
  subtitle?: string;
  nav: AdminNavItem[];
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

export function AdminShell({ title, subtitle, nav, actions, children, className }: AdminShellProps) {
  return (
    <div className={cn('flex min-h-[760px] w-full overflow-hidden rounded-2xl border border-ink-950/10 bg-paper-100 shadow-panel', className)}>
      <aside className="hidden w-[72px] shrink-0 flex-col bg-ink-950 px-2 py-5 text-paper-50 transition-[width] md:flex xl:w-60 xl:px-4">
        <div className="flex items-center justify-center gap-3 xl:justify-start xl:px-2">
          <span className="grid size-9 place-items-center rounded-lg bg-copper-500"><Gavel size={18} /></span>
          <div className="hidden xl:block">
            <div className="text-sm font-bold tracking-[0.12em]">GAVEL</div>
            <div className="text-[10px] uppercase tracking-[0.14em] text-paper-200/50">operations</div>
          </div>
        </div>
        <nav aria-label="Основная навигация" className="mt-9 space-y-1">
          {nav.map((item) => (
            <button
              key={item.label}
              onClick={item.onClick}
              aria-label={item.label}
              title={item.label}
              className={cn(
                'flex w-full items-center justify-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm transition-colors xl:justify-start',
                item.active ? 'bg-paper-50 text-ink-950' : 'text-paper-200/70 hover:bg-white/7 hover:text-white',
              )}
            >
              <span className="size-4">{item.icon}</span>
              <span className="hidden flex-1 xl:block">{item.label}</span>
              {item.badge && <span className="hidden rounded bg-copper-500 px-1.5 py-0.5 text-[10px] font-bold text-white xl:inline">{item.badge}</span>}
            </button>
          ))}
        </nav>
        <div className="mt-auto border-t border-white/10 px-1 pt-4 xl:px-2">
          <div className="mx-auto grid size-8 place-items-center rounded-full bg-white/10 text-[10px] font-bold xl:hidden">EV</div>
          <div className="hidden xl:block"><p className="m-0 text-xs font-medium">Elena Volkova</p><p className="mt-1 mb-0 text-[10px] uppercase tracking-wider text-paper-200/65">Senior operator</p></div>
        </div>
      </aside>
      <main className="min-w-0 flex-1">
        <header className="flex min-h-20 flex-wrap items-center justify-between gap-3 border-b border-paper-200 bg-paper-50 px-4 py-3 lg:px-6 xl:px-7">
          <div>
            <h1 className="m-0 text-xl font-semibold tracking-[-0.03em] text-ink-950">{title}</h1>
            {subtitle && <p className="mt-1 mb-0 text-xs text-ink-500">{subtitle}</p>}
          </div>
          <div className="flex items-center gap-2">
            <button aria-label="Поиск" className="grid size-10 place-items-center rounded-lg border border-paper-200 bg-white text-ink-700 hover:border-ink-500"><Search size={17} /></button>
            {actions}
          </div>
        </header>
        <nav aria-label="Мобильная навигация" className="flex overflow-x-auto border-b border-paper-200 bg-paper-50 px-2 md:hidden">
          {nav.map((item) => (
            <button
              key={item.label}
              onClick={item.onClick}
              className={cn(
                'relative flex shrink-0 items-center gap-2 px-3 py-3 text-xs font-semibold',
                item.active ? 'text-ink-950' : 'text-ink-500',
              )}
            >
              {item.icon}
              {item.label}
              {item.active && <span className="absolute right-2 bottom-0 left-2 h-0.5 bg-copper-500" />}
            </button>
          ))}
        </nav>
        <div className="p-4 lg:p-6 xl:p-7">{children}</div>
      </main>
    </div>
  );
}
