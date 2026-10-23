import { useEffect, useRef, useState } from 'react';
import { cn } from '../lib/cn';

export interface LineChartProps {
  /** One label per value: an x position's name. */
  labels: string[];
  values: number[];
  /** Indices to label on the x axis; keeps one step. Defaults to five evenly spaced. */
  xAt?: number[];
  format?: (value: number) => string;
  height?: number;
  /** The one chart hue goes on the main data element; everything else in ink. */
  hue?: 'ink' | 'copper';
  /** The event that matters: a 1px marker with its cause. */
  annotations?: Array<{ at: number; label: string }>;
  /** A dashed reference line with its label at the end. */
  reference?: { value: number; label: string };
  ariaLabel: string;
  className?: string;
}

/* Round ticks (1, 2, 2.5, 5 × 10^n) whose top sits above the largest value. */
function ticks(min: number, max: number, count = 4) {
  if (max === min) max = min + 1;
  const raw = (max - min) / count;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) || 10 * mag;
  const lo = Math.floor(min / step) * step;
  let hi = Math.ceil(max / step) * step;
  if (hi <= max) hi += step;
  const out: number[] = [];
  for (let v = lo; v <= hi + step / 2; v += step) out.push(+v.toFixed(10));
  return out;
}

/*
  One series, straight 2px segments, no fill, no markers; 3–5 faint horizontal gridlines, round y ticks
  at 12px in a lane on the left, flat x labels on one step; an 8px hover dot with a bare tooltip.
*/
export function LineChart({ labels, values, xAt, format = (v) => String(v), height = 220, hue = 'ink', annotations = [], reference, ariaLabel, className }: LineChartProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(640);
  const [hover, setHover] = useState<number | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setWidth(Math.max(240, Math.round(entry.contentRect.width))));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const laneTop = annotations.length ? 22 : 8;
  const pad = { top: laneTop, right: 12, bottom: 24, left: 44 };
  const plotW = width - pad.left - pad.right;
  const plotH = height - pad.top - pad.bottom;
  const min = Math.min(0, ...values);
  const max = Math.max(...values, reference?.value ?? 0);
  const yTicks = ticks(min, max, 4);
  const yMin = yTicks[0];
  const yMax = yTicks[yTicks.length - 1];
  const x = (i: number) => pad.left + (values.length > 1 ? (i / (values.length - 1)) * plotW : plotW / 2);
  const y = (v: number) => pad.top + plotH - ((v - yMin) / (yMax - yMin)) * plotH;
  const path = values.map((v, i) => `${i === 0 ? 'M' : 'L'}${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join(' ');
  const xLabels = xAt ?? [0, 1, 2, 3, 4].map((k) => Math.round((k * (values.length - 1)) / 4));
  const stroke = hue === 'copper' ? 'var(--color-copper)' : 'var(--color-ink)';

  const onMove = (event: React.MouseEvent<SVGSVGElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const px = event.clientX - rect.left - pad.left;
    const i = Math.round((px / plotW) * (values.length - 1));
    setHover(Math.min(values.length - 1, Math.max(0, i)));
  };

  return (
    <div ref={ref} className={cn('relative min-w-0', className)}>
      <svg role="img" aria-label={ariaLabel} width={width} height={height} className="block max-w-full" onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
        {yTicks.map((tick) => (
          <g key={tick}>
            <line x1={pad.left} x2={width - pad.right} y1={y(tick)} y2={y(tick)} stroke="var(--color-rule)" strokeWidth={1} />
            <text x={pad.left - 8} y={y(tick) + 4} textAnchor="end" fontSize={12} fill="var(--color-faint)" className="tabular-nums">{format(tick)}</text>
          </g>
        ))}
        {reference && (
          <g>
            <line x1={pad.left} x2={width - pad.right} y1={y(reference.value)} y2={y(reference.value)} stroke="var(--color-control)" strokeWidth={1} strokeDasharray="3 3" />
            <text x={width - pad.right} y={y(reference.value) - 4} textAnchor="end" fontSize={12} fill="var(--color-muted)">{reference.label}</text>
          </g>
        )}
        {annotations.map((note) => (
          <g key={note.at}>
            <line x1={x(note.at)} x2={x(note.at)} y1={pad.top} y2={pad.top + plotH} stroke="var(--color-control)" strokeWidth={1} />
            <text x={Math.min(x(note.at) + 4, width - pad.right - 4)} y={12} textAnchor={x(note.at) > width * 0.7 ? 'end' : 'start'} fontSize={12} fill="var(--color-muted)">{note.label}</text>
          </g>
        ))}
        <path d={path} fill="none" stroke={stroke} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
        {xLabels.map((i) => (
          <text key={i} x={x(i)} y={height - 6} textAnchor={i === 0 ? 'start' : i === values.length - 1 ? 'end' : 'middle'} fontSize={12} fill="var(--color-faint)">{labels[i]}</text>
        ))}
        {hover !== null && (
          <g>
            <line x1={x(hover)} x2={x(hover)} y1={pad.top} y2={pad.top + plotH} stroke="var(--color-control)" strokeWidth={1} strokeDasharray="3 3" />
            <circle cx={x(hover)} cy={y(values[hover])} r={4} fill={stroke} />
          </g>
        )}
      </svg>
      {hover !== null && (
        <div className="pointer-events-none absolute top-0 rounded-[4px] border border-line bg-surface px-2 py-1 text-xs whitespace-nowrap text-ink tabular-nums" style={{ left: Math.min(x(hover) + 8, width - 140) }}>
          {labels[hover]} · {format(values[hover])}
        </div>
      )}
    </div>
  );
}
