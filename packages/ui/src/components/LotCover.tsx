import { Archive, Armchair, Camera, Image, ShoppingBag, Watch } from 'lucide-react';
import type { LotCover as Kind } from '../data/ops';
import { cn } from '../lib/cn';

const icons = { watch: Watch, chair: Armchair, camera: Camera, print: Image, cabinet: Archive, bag: ShoppingBag };

/* A flat slot where the lot's photograph will go: the raised tone with one grey outline glyph of the object's kind. Never a gradient. */
export function LotCover({ kind, label, className, iconSize = 28 }: { kind: Kind; label?: string; className?: string; iconSize?: number }) {
  const Icon = icons[kind];
  return (
    <div className={cn('relative grid place-items-center overflow-hidden rounded-[6px] bg-raised text-control', className)} role="img" aria-label={label ?? 'No photograph yet'}>
      <Icon size={iconSize} strokeWidth={1.25} aria-hidden="true" />
      {label && <span className="absolute bottom-2 left-2 text-xs text-muted">{label}</span>}
    </div>
  );
}
