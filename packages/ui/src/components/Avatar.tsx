import { cn } from '../lib/cn';
import { initials } from '../data/customers';

/* People get a stable initial on a pale tint of their own hue. Identity, not data: pale, and never in a chart legend. */
export function Avatar({ name, hue = 20, size = 'md', className }: { name: string; hue?: number; size?: 'sm' | 'md' | 'lg'; className?: string }) {
  const dims = { sm: 'size-6 text-[11px]', md: 'size-8 text-xs', lg: 'size-12 text-base' }[size];
  return (
    <span aria-hidden="true" className={cn('grid shrink-0 place-items-center rounded-full font-medium', dims, className)} style={{ background: `hsl(${hue} 45% 92%)`, color: `hsl(${hue} 45% 28%)` }}>
      {initials(name)}
    </span>
  );
}
