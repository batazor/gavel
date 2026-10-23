import { useState } from 'react';
import { Button } from '../components/Button';
import { ClosingBoard } from '../components/ClosingBoard';
import { LineChart } from '../components/LineChart';
import { Kpi, KpiStrip } from '../components/MetricCard';
import { Panel } from '../components/Panel';
import { cn } from '../lib/cn';
import { formatClock, formatDuration, formatInt, formatMoney, formatSigned } from '../lib/format';
import { activeBidders, bidsLabels, bidsNow, bidsPerMinute, bidsSpike, bidsYesterday, closingSoon, hammerVolume, hammerVolumeSeries, health, hotLots, liveAuctions, liveAuctionsSeries, queue, queueTimeCritical, queueTotal, today, type QueueItem } from '../data/ops';
import { OpsPageShell, type OpsSection } from './PageShell';

export interface AdminOverviewPageProps {
  onNavigate?: (section: OpsSection) => void;
  onOpenMonitor?: () => void;
  onOpenInbox?: () => void;
  onOpenObservability?: () => void;
  onOpenSettlement?: () => void;
  onOpenCustomer?: () => void;
  onOpenLot?: () => void;
}

type Series = 'bids' | 'live' | 'hammer';

const series: Record<Series, { label: string; values: number[]; format: (v: number) => string; annotations?: Array<{ at: number; label: string }> }> = {
  bids: { label: 'Accepted bids per minute', values: bidsPerMinute, format: formatInt, annotations: [{ at: bidsSpike.at, label: bidsSpike.label }] },
  live: { label: 'Live auctions', values: liveAuctionsSeries, format: formatInt },
  hammer: { label: 'Hammer volume since midnight', values: hammerVolumeSeries, format: (v) => formatMoney(v, 'EUR', 0) },
};

const critical = queue.find((item) => item.tone === 'danger');

/*
  The screen's question: is anything waiting on an operator right now, and are the live auctions healthy?
  The biggest block is the bids chart with the three platform KPIs as its tabs; the finding is the line under the title;
  the most frequent job (open the queue) is the primary button. The closing board is the house's own element.
  Grid at 1440: canvas 1136 = 2 × 560 + 16.
*/
export function AdminOverviewPage({ onNavigate, onOpenMonitor, onOpenInbox, onOpenObservability, onOpenSettlement, onOpenCustomer, onOpenLot }: AdminOverviewPageProps = {}) {
  const [selected, setSelected] = useState<Series>('bids');
  const bidsChange = bidsNow - bidsYesterday;
  const liveChange = liveAuctions.now - liveAuctions.yesterday;
  const hammerChange = hammerVolume.now - hammerVolume.yesterday;
  const finding = critical
    ? `${critical.kind} ${critical.id} expires at ${formatClock(critical.dueAt!)}, ${formatDuration(critical.dueAt! - today.nowMinutes)} from now. ${queueTimeCritical} of ${queueTotal} open cases is time-critical; ${liveAuctions.now} auctions are live.`
    : `${queueTotal} open cases, none time-critical; ${liveAuctions.now} auctions are live.`;
  const openQueue = (item: QueueItem) => {
    if (item.target === 'settlements') onOpenSettlement?.();
    else if (item.target === 'customers') onOpenCustomer?.();
    else if (item.target === 'lots') onOpenLot?.();
    else onNavigate?.('lots');
  };

  return (
    <OpsPageShell
      section="overview"
      onNavigate={onNavigate}
      title="Overview"
      subtitle={finding}
      actions={<>
        <Button variant="secondary" onClick={onOpenMonitor}>Live monitor</Button>
        <Button onClick={onOpenInbox}>Open inbox<span className="tabular-nums">{queueTotal}</span></Button>
      </>}
    >
      <Panel title="Today against yesterday" meta={`${today.date} · ${today.nowLabel}`}>
        <div role="tablist" aria-label="Series" className="grid grid-cols-2 border-y border-line sm:grid-cols-3">
          <KpiTab id="bids" selected={selected} onSelect={setSelected} label="Bids per minute" value={formatInt(bidsNow)} change={`${formatSigned(bidsChange)} vs yesterday ${today.nowLabel.slice(0, 5)}`} good={bidsChange >= 0} lead />
          <KpiTab id="live" selected={selected} onSelect={setSelected} label="Live auctions" value={formatInt(liveAuctions.now)} change={`${formatSigned(liveChange)} vs yesterday ${today.nowLabel.slice(0, 5)}`} good={liveChange >= 0} />
          <KpiTab id="hammer" selected={selected} onSelect={setSelected} label="Hammer volume today" value={formatMoney(hammerVolume.now)} change={`${formatSigned(hammerChange, (v) => formatMoney(v))} vs yesterday ${today.nowLabel.slice(0, 5)}`} good={hammerChange >= 0} money className="col-span-2 sm:col-span-1" />
        </div>
        <div className="px-5 pt-4 pb-2">
          <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
            <h3 className="text-sm font-medium text-ink">{series[selected].label}</h3>
            <span className="text-xs text-faint">{formatClock(bidsLabels[0])}–{formatClock(bidsLabels[bidsLabels.length - 1])} UTC, one value per minute</span>
          </div>
          <LineChart
            className="mt-2"
            ariaLabel={`${series[selected].label} over the last 60 minutes`}
            labels={bidsLabels.map(formatClock)}
            values={series[selected].values}
            xAt={[0, 15, 30, 45, 60]}
            format={series[selected].format}
            hue="copper"
            height={220}
            annotations={series[selected].annotations}
          />
        </div>
        <table className="mx-5 mb-4 w-[calc(100%-40px)] text-sm">
          <thead>
            <tr className="text-xs text-muted">
              <th className="h-9 text-left font-normal">Now against yesterday, same minute</th>
              <th className="h-9 text-right font-normal">Today</th>
              <th className="hidden h-9 text-right font-normal sm:table-cell">Yesterday</th>
              <th className="h-9 text-right font-normal">Change</th>
            </tr>
          </thead>
          <tbody>
            <CompareRow label="Bidders active in the last 30 minutes" today={formatInt(activeBidders)} yesterday="—" change="—" />
            <CompareRow label="Peak bids per minute" today={formatInt(Math.max(...bidsPerMinute))} yesterday={formatInt(2_640)} change={formatSigned(Math.max(...bidsPerMinute) - 2_640)} good />
            <CompareRow label="Lots closed since midnight" today="42" yesterday="47" change={formatSigned(42 - 47)} good={false} />
          </tbody>
        </table>
      </Panel>

      <div className="mt-4 grid items-start gap-4 lg:grid-cols-2">
        <Panel title="Closing in the next hour" meta={`${closingSoon.length} lots`}>
          <div className="px-5 pb-4">
            <ClosingBoard now={today.nowMinutes} rows={closingSoon.map((lot) => ({ id: lot.id, title: lot.title, price: lot.currentPrice, scheduledEnd: lot.scheduledEnd!, extendedTo: lot.extendedTo, onOpen: onOpenLot }))} />
          </div>
        </Panel>
        <Panel title="Needs an operator" meta={`${queueTotal} open · ${queueTimeCritical} time-critical`} action={<button type="button" onClick={onOpenInbox} className="text-sm font-medium text-ink underline-offset-[3px] hover:underline">View all {queueTotal}</button>}>
          <ol className="divide-y divide-rule border-t border-rule">
            {queue.slice(0, 4).map((item) => (
              <li key={item.id}>
                <button type="button" onClick={() => openQueue(item)} className="grid min-h-12 w-full grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-3 px-5 py-2.5 text-left hover:bg-raised">
                  <span aria-hidden="true" className={cn('size-2 rounded-full', item.tone === 'danger' ? 'bg-negative' : item.tone === 'warning' ? 'bg-warning' : 'bg-control')} />
                  <span className="min-w-0">
                    <span className="block truncate text-sm text-ink">{item.kind} <span className="text-muted">· {item.object}</span></span>
                    <span className="block truncate text-xs text-muted">{item.detail}</span>
                  </span>
                  <span className={cn('text-right text-xs tabular-nums', item.tone === 'danger' ? 'font-medium text-negative' : 'text-muted')}>
                    {item.dueAt !== null ? `${formatDuration(item.dueAt - today.nowMinutes)} left` : 'No deadline'}
                  </span>
                </button>
              </li>
            ))}
          </ol>
        </Panel>
      </div>

      <div className="mt-4 grid items-start gap-4 lg:grid-cols-2">
        <Panel title="Hottest live lots" meta="by accepted bids" action={<button type="button" onClick={() => onNavigate?.('lots')} className="text-sm font-medium text-ink underline-offset-[3px] hover:underline">All lots</button>}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-t border-rule text-xs text-muted">
                <th className="h-9 pl-5 text-left font-normal">Lot</th>
                <th className="h-9 text-right font-normal">Bids</th>
                <th className="hidden h-9 text-right font-normal sm:table-cell">Watching</th>
                <th className="h-9 pr-5 text-right font-normal">Price</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-rule border-t border-rule">
              {hotLots.map((lot) => (
                <tr key={lot.id} onClick={onOpenLot} className={cn('h-11', onOpenLot && 'cursor-pointer hover:bg-raised')}>
                  <td className="max-w-0 truncate pl-5"><span className="text-ink">{lot.title}</span><span className="ml-2 font-mono text-xs text-faint">{lot.id}</span></td>
                  <td className="text-right tabular-nums">{formatInt(lot.bids)}</td>
                  <td className="hidden text-right text-muted tabular-nums sm:table-cell">{formatInt(lot.watching)}</td>
                  <td className="pr-5 text-right font-bold tabular-nums">{formatMoney(lot.currentPrice)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
        <Panel title="Platform health" meta="last 15 minutes" action={<button type="button" onClick={onOpenObservability} className="text-sm font-medium text-ink underline-offset-[3px] hover:underline">Observability</button>}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-t border-rule text-xs text-muted">
                <th className="h-9 pl-5 text-left font-normal">Critical path</th>
                <th className="hidden h-9 text-left font-normal sm:table-cell">Detail</th>
                <th className="h-9 pr-5 text-right font-normal">Now</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-rule border-t border-rule">
              {health.map((row) => (
                <tr key={row.name} className="h-11">
                  <td className="pl-5 text-ink">{row.name}</td>
                  <td className="hidden text-xs text-muted sm:table-cell">{row.detail}</td>
                  <td className={cn('pr-5 text-right tabular-nums', row.ok ? 'text-ink' : 'font-medium text-negative')}>{row.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      </div>
    </OpsPageShell>
  );
}

function KpiTab({ id, selected, onSelect, label, value, change, good, money, lead, className }: { id: Series; selected: Series; onSelect: (id: Series) => void; label: string; value: string; change: string; good: boolean; money?: boolean; lead?: boolean; className?: string }) {
  const active = id === selected;
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={() => onSelect(id)}
      className={cn('relative text-left [&>*]:px-5 sm:[&+&]:border-l sm:[&+&]:border-line max-sm:[&:nth-child(2)]:border-l max-sm:[&:nth-child(2)]:border-line max-sm:[&:nth-child(n+3)]:border-t max-sm:[&:nth-child(n+3)]:border-line', active ? 'after:absolute after:right-0 after:bottom-[-1px] after:left-0 after:h-0.5 after:bg-ink' : 'hover:bg-raised', className)}
    >
      <Kpi label={label} value={value} change={change} changeGood={good} money={money} lead={lead} className={cn(!active && '[&>p:nth-child(2)]:font-normal')} />
    </button>
  );
}

function CompareRow({ label, today, yesterday, change, good }: { label: string; today: string; yesterday: string; change: string; good?: boolean }) {
  return (
    <tr className="border-t border-rule">
      <td className="h-9 text-ink">{label}</td>
      <td className="h-9 text-right text-ink tabular-nums">{today}</td>
      <td className="hidden h-9 text-right text-muted tabular-nums sm:table-cell">{yesterday}</td>
      <td className={cn('h-9 text-right tabular-nums', change === '—' ? 'text-faint' : good === undefined ? 'text-muted' : good ? 'text-positive' : 'text-negative')}>{change}</td>
    </tr>
  );
}
