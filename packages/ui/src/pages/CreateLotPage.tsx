import { useState, type ReactElement } from 'react';
import { Button } from '../components/Button';
import { ClosingPolicyBuilder } from '../components/ClosingPolicyBuilder';
import { GavelMark } from '../components/GavelMark';
import { LotCover } from '../components/LotCover';
import { StatusBadge } from '../components/StatusBadge';
import { cn } from '../lib/cn';
import { formatMoney } from '../lib/format';

const steps = ['Media', 'Details', 'Pricing', 'Review'] as const;
type Step = (typeof steps)[number];

/*
  The seller studio: the screen's question is "is this lot ready to publish?". The steps are underline tabs,
  the form is one sheet with labels above 40px fields, and the preview on the right shows what bidders will see.
*/
export function CreateLotPage({ onBackToLots, onPublished }: { onBackToLots?: () => void; onPublished?: () => void } = {}) {
  const [step, setStep] = useState<Step>('Media');
  const [title, setTitle] = useState('Omega Speedmaster Professional, 1969');
  const [startingPrice, setStartingPrice] = useState(18_000);
  const [bidStep, setBidStep] = useState(200);
  const [published, setPublished] = useState(false);
  const index = steps.indexOf(step);
  const publish = () => { setPublished(true); onPublished?.(); };

  return (
    <div className="flex h-full min-h-0 flex-col bg-ground">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line px-5 sm:px-6">
        <button type="button" onClick={onBackToLots} className="flex items-center gap-2 text-sm font-semibold text-ink"><GavelMark size={20} className="text-copper" />Gavel</button>
        <span className="text-sm text-muted">Seller studio</span>
        <span className="ml-auto hidden font-mono text-xs text-muted sm:inline">Draft LOT-D92F</span>
        <Button variant="ghost" size="sm">Save draft</Button>
        {step === 'Review' ? <Button size="sm" onClick={publish} disabled={published}>{published ? 'Published' : 'Publish lot'}</Button> : <Button size="sm" variant="secondary" onClick={() => setStep('Review')}>Go to review</Button>}
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        <div className="mx-auto w-full max-w-[1100px] px-5 py-5 sm:px-6 sm:py-6">
          <div className="flex flex-wrap items-end justify-between gap-3">
            <div>
              <h1 className="text-2xl font-semibold tracking-[-0.01em] text-ink">New lot</h1>
              <p className="mt-1 text-sm text-muted">{published ? 'Listed. The catalogue accepted the lot; the auction opens from the event stream.' : `${index + 1} of ${steps.length} steps · the description and the starting price freeze after the first accepted bid.`}</p>
            </div>
            {published && <StatusBadge tone="success" dot>Listed</StatusBadge>}
          </div>

          <div role="tablist" aria-label="Steps" className="mt-5 flex gap-6 overflow-x-auto border-b border-line">
            {steps.map((item, i) => (
              <button key={item} role="tab" aria-selected={step === item} onClick={() => setStep(item)} className={cn('relative shrink-0 py-2.5 text-sm transition-colors', step === item ? 'text-ink after:absolute after:right-0 after:bottom-[-1px] after:left-0 after:h-0.5 after:bg-ink' : 'text-muted hover:text-ink')}>
                {item}{i < index && <span className="ml-1.5 text-xs text-positive">done</span>}
              </button>
            ))}
          </div>

          <div className="mt-5 grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
            <section className="rounded-[8px] border border-line bg-surface">
              {step === 'Media' && <MediaStep />}
              {step === 'Details' && <DetailsStep title={title} setTitle={setTitle} />}
              {step === 'Pricing' && <PricingStep startingPrice={startingPrice} setStartingPrice={setStartingPrice} bidStep={bidStep} setBidStep={setBidStep} />}
              {step === 'Review' && <ReviewStep title={title} startingPrice={startingPrice} bidStep={bidStep} />}
              <footer className="flex items-center justify-between gap-3 border-t border-line px-5 py-3">
                <Button variant="ghost" disabled={index === 0} onClick={() => setStep(steps[Math.max(0, index - 1)])}>Back</Button>
                {step !== 'Review' ? <Button onClick={() => setStep(steps[index + 1])}>Continue to {steps[index + 1]}</Button> : <Button onClick={publish} disabled={published}>{published ? 'Published' : 'Publish lot'}</Button>}
              </footer>
            </section>
            <LotPreview title={title} startingPrice={startingPrice} />
          </div>
        </div>
      </main>
    </div>
  );
}

function StepHeader({ title, description }: { title: string; description: string }) {
  return <div className="px-5 pt-5"><h2 className="text-lg font-semibold text-ink">{title}</h2><p className="mt-1 text-sm text-muted">{description}</p></div>;
}

const guide = [
  { shot: 'Front', why: 'the cover', done: true, required: true },
  { shot: 'Dial', why: 'patina and lume', done: true, required: true },
  { shot: 'Caseback', why: 'reference and serial', done: true, required: true },
  { shot: 'Movement', why: 'calibre, open caseback', done: false, required: true },
  { shot: 'Bracelet and clasp', why: 'stretch and end links', done: false, required: false },
  { shot: 'Box and papers', why: 'archive extract', done: false, required: false },
];

function MediaStep() {
  const images = ['Front', 'Dial', 'Caseback'];
  return (
    <div>
      <StepHeader title="Photographs" description="Up to 20 images. The first one is the cover bidders see in the catalogue." />
      <div className="p-5">
        <button type="button" className="grid w-full place-items-center rounded-[6px] border border-dashed border-control bg-surface px-5 py-8 text-center hover:bg-raised">
          <span className="text-sm font-medium text-ink">Drop photographs here, or browse</span>
          <span className="mt-1 text-xs text-muted">JPEG, PNG or WebP · up to 20 MB each</span>
        </button>
        <ul className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
          {images.map((label, i) => (
            <li key={label} className="relative">
              <LotCover kind="watch" className="aspect-square" label={label} />
              {i === 0 && <StatusBadge className="absolute top-2 left-2">Cover</StatusBadge>}
            </li>
          ))}
          <li><button type="button" className="grid aspect-square w-full place-items-center rounded-[6px] border border-dashed border-control text-sm text-muted hover:bg-raised">Add image</button></li>
        </ul>
        <p className="mt-3 text-xs text-muted">{images.length} of 20 images · all uploads processed and owned by this seller</p>
        <h3 className="mt-6 text-sm font-medium text-ink">What a watch listing needs</h3>
        <ol className="mt-1 divide-y divide-rule">
          {guide.map((row) => (
            <li key={row.shot} className="flex h-10 items-center gap-3 text-sm">
              <span aria-hidden="true" className={cn('size-2 shrink-0 rounded-full', row.done ? 'bg-positive' : row.required ? 'border border-control bg-surface' : 'bg-control')} />
              <span className="min-w-0 flex-1 truncate text-ink">{row.shot}<span className="ml-2 text-xs text-muted">{row.why}</span></span>
              <span className={cn('text-xs', row.done ? 'text-muted' : row.required ? 'text-ink' : 'text-faint')}>{row.done ? 'Uploaded' : row.required ? 'Required' : 'Optional'}</span>
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}

function DetailsStep({ title, setTitle }: { title: string; setTitle: (value: string) => void }) {
  return (
    <div>
      <StepHeader title="Describe the lot" description="What it is, its condition and where it is. The description freezes after the first accepted bid." />
      <div className="grid gap-4 p-5 sm:grid-cols-2">
        <Field label="Title" className="sm:col-span-2"><input value={title} onChange={(event) => setTitle(event.target.value)} /></Field>
        <Field label="Category"><select defaultValue="watches"><option value="watches">Watches</option><option value="art">Art</option><option value="design">Design</option><option value="cameras">Cameras</option></select></Field>
        <Field label="Condition"><select defaultValue="very-good"><option value="very-good">Very good</option><option value="excellent">Excellent</option><option value="fair">Fair</option></select></Field>
        <Field label="Description" className="sm:col-span-2"><textarea rows={5} defaultValue="Calibre 861, original stepped dial, serviced in 2025. Period-correct bracelet and an archive extract are included." /></Field>
        <Field label="Location"><input defaultValue="Berlin, Germany" /></Field>
        <Field label="Seller reference"><input defaultValue="WATCH-1969-042" /></Field>
      </div>
    </div>
  );
}

function PricingStep({ startingPrice, setStartingPrice, bidStep, setBidStep }: { startingPrice: number; setStartingPrice: (v: number) => void; bidStep: number; setBidStep: (v: number) => void }) {
  return (
    <div>
      <StepHeader title="Price and closing" description="The opening price, the bid step and how the auction closes." />
      <div className="grid gap-4 p-5 sm:grid-cols-2">
        <Field label="Starting price, €"><input type="number" min={0} step={100} value={startingPrice} onChange={(event) => setStartingPrice(Number(event.target.value))} className="tabular-nums" /></Field>
        <Field label="Bid step, €"><input type="number" min={10} step={10} value={bidStep} onChange={(event) => setBidStep(Number(event.target.value))} className="tabular-nums" /></Field>
      </div>
      <div className="px-5 pb-5"><ClosingPolicyBuilder initialValue={{ scheduledEnd: '2026-10-24T18:00', inactivityMinutes: 5, hardDeadline: '2026-10-24T20:00' }} /></div>
    </div>
  );
}

function ReviewStep({ title, startingPrice, bidStep }: { title: string; startingPrice: number; bidStep: number }) {
  const checks = ['3 photographs processed and ordered', 'Required lot details complete', `Starting price ${formatMoney(startingPrice)}, bid step ${formatMoney(bidStep)}`, 'Closing policy has a hard deadline', 'Seller permission verified'];
  return (
    <div>
      <StepHeader title="Ready to publish" description="These inputs cannot change once the auction has a bid." />
      <div className="p-5">
        <div className="rounded-[6px] bg-raised px-4 py-3"><p className="text-sm font-medium text-ink">{title}</p><p className="text-xs text-muted">Watches · Berlin, Germany · no reserve</p></div>
        <ul className="mt-4 divide-y divide-rule">
          {checks.map((check) => <li key={check} className="flex h-10 items-center gap-3 text-sm text-ink"><span aria-hidden="true" className="size-2 rounded-full bg-positive" />{check}</li>)}
        </ul>
        <p className="mt-4 text-xs text-muted">Publishing commits the lot and its listing event together; the auction opens asynchronously and idempotently.</p>
      </div>
    </div>
  );
}

function LotPreview({ title, startingPrice }: { title: string; startingPrice: number }) {
  return (
    <aside className="rounded-[8px] border border-line bg-surface p-4 lg:sticky lg:top-0">
      <div className="flex items-center justify-between"><h2 className="text-base font-medium text-ink">What bidders see</h2><StatusBadge>Draft</StatusBadge></div>
      <div className="mt-4 grid gap-4 sm:grid-cols-[200px_minmax(0,1fr)] lg:grid-cols-1">
        <LotCover kind="watch" className="aspect-[4/3] sm:aspect-square lg:aspect-[4/3]" iconSize={40} />
        <div className="min-w-0">
          <p className="text-xs text-muted">Watches</p>
          <p className="mt-0.5 text-base font-medium text-ink">{title || 'Untitled lot'}</p>
          <p className="mt-1 text-sm text-muted">Calibre 861, original stepped dial, serviced in 2025.</p>
          <div className="mt-4 border-t border-rule pt-3">
            <p className="text-xs text-muted">Starts at</p>
            <p className="text-2xl font-bold text-copper tabular-nums">{formatMoney(startingPrice)}</p>
          </div>
        </div>
      </div>
    </aside>
  );
}

function Field({ label, children, className }: { label: string; children: ReactElement<{ className?: string }>; className?: string }) {
  return (
    <label className={cn('block text-sm font-medium text-ink', className)}>
      {label}
      <span className="mt-2 block [&>*]:w-full [&>*]:rounded-[6px] [&>*]:border [&>*]:border-control [&>*]:bg-surface [&>*]:px-3 [&>*]:py-2 [&>*]:text-sm [&>*]:font-normal [&>*]:text-ink [&>*]:outline-none [&>*]:focus:border-ink [&>input]:h-10 [&>select]:h-10">{children}</span>
    </label>
  );
}
