import { useState, type ReactElement } from 'react';
import { ArrowLeft, ArrowRight, Check, CircleDollarSign, FileText, Gavel, GripVertical, ImagePlus, Images, PackageCheck, Save, Sparkles, UploadCloud } from 'lucide-react';
import { Button } from '../components/Button';
import { ClosingPolicyBuilder } from '../components/ClosingPolicyBuilder';
import { StatusBadge } from '../components/StatusBadge';
import { cn } from '../lib/cn';

const steps = [
  { label: 'Media', icon: Images },
  { label: 'Details', icon: FileText },
  { label: 'Pricing', icon: CircleDollarSign },
  { label: 'Review', icon: PackageCheck },
];

export function CreateLotPage({ onBackToLots, onPublished }: { onBackToLots?: () => void; onPublished?: () => void } = {}) {
  const [step, setStep] = useState(0);
  const [title, setTitle] = useState('Omega Speedmaster Professional, 1969');
  const [published, setPublished] = useState(false);
  return <div className="min-h-[860px] bg-paper-100">
    <header className="flex min-h-16 items-center border-b border-paper-200 bg-paper-50 px-5 lg:px-8"><button onClick={onBackToLots} className="flex items-center gap-2 font-bold tracking-[0.1em]"><Gavel size={18} className="text-copper-500" /> GAVEL</button><span className="mx-4 h-5 w-px bg-paper-200" /><span className="text-xs text-ink-500">Seller studio</span><div className="ml-auto flex items-center gap-2"><Button variant="ghost" size="sm" leadingIcon={<Save size={14} />}>Save draft</Button><Button size="sm" onClick={() => { if (step < 3) { setStep(3); return; } setPublished(true); onPublished?.(); }}>{published ? 'Published' : step < 3 ? 'Review & publish' : 'Publish lot'}</Button></div></header>
    <div className="border-b border-paper-200 bg-paper-50 px-5 lg:px-8"><nav aria-label="Create lot progress" className="mx-auto flex max-w-6xl overflow-x-auto">{steps.map((item, index) => { const Icon = item.icon; const current = index === step; const complete = index < step || published; return <button key={item.label} onClick={() => setStep(index)} className={cn('relative flex min-w-32 flex-1 items-center justify-center gap-2 px-4 py-4 text-xs font-semibold', current ? 'text-ink-950' : 'text-ink-500')}><span className={cn('grid size-6 place-items-center rounded-full border', complete ? 'border-moss-500 bg-moss-500 text-white' : current ? 'border-copper-500 bg-copper-500 text-white' : 'border-paper-200 bg-white')}>{complete ? <Check size={12} /> : <Icon size={12} />}</span>{index + 1}. {item.label}{current && <span className="absolute right-0 bottom-0 left-0 h-0.5 bg-copper-500" />}</button>; })}</nav></div>
    <main className="mx-auto max-w-6xl px-5 py-7 lg:px-8">
      {published && <div role="status" className="mb-5 flex items-center gap-3 rounded-xl border border-moss-500/25 bg-moss-500/10 p-4 text-sm text-moss-600"><PackageCheck size={18} /><strong>Lot listed.</strong><span>Catalog accepted the lot; Auction will open it from the event stream.</span></div>}
      <div className="grid gap-6 lg:grid-cols-[1fr_310px]">
        <section className="overflow-hidden rounded-2xl border border-paper-200 bg-paper-50 shadow-panel">
          {step === 0 && <MediaStep />}
          {step === 1 && <DetailsStep title={title} setTitle={setTitle} />}
          {step === 2 && <PricingStep />}
          {step === 3 && <ReviewStep title={title} />}
          <footer className="flex items-center justify-between border-t border-paper-200 px-5 py-4"><Button variant="ghost" disabled={step === 0} onClick={() => setStep((current) => Math.max(0, current - 1))} leadingIcon={<ArrowLeft size={14} />}>Back</Button><div className="font-mono text-[10px] text-ink-500">Draft LOT-D92F</div>{step < 3 ? <Button onClick={() => setStep((current) => Math.min(3, current + 1))}>Continue <ArrowRight size={14} /></Button> : <Button onClick={() => { setPublished(true); onPublished?.(); }}>Publish lot</Button>}</footer>
        </section>
        <LotPreview title={title} step={step} />
      </div>
    </main>
  </div>;
}

function MediaStep() {
  return <div><div className="border-b border-paper-200 p-5"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Step 1</p><h1 className="mt-1 mb-1 text-2xl font-semibold tracking-[-0.03em]">Add images</h1><p className="m-0 text-xs text-ink-500">Upload 1–20 images. The first image becomes the cover.</p></div><div className="p-5"><button className="flex w-full flex-col items-center rounded-xl border-2 border-dashed border-paper-200 bg-paper-100 px-5 py-10 text-center hover:border-copper-400"><span className="grid size-12 place-items-center rounded-full bg-paper-50 text-copper-600 shadow-sm"><UploadCloud size={22} /></span><strong className="mt-3 text-sm">Drop photos here or browse</strong><span className="mt-1 text-xs text-ink-500">JPEG, PNG or WebP · up to 20 MB each</span></button><div className="mt-5 grid grid-cols-2 gap-3 sm:grid-cols-4"><MediaTile cover label="Front" /><MediaTile label="Dial" /><MediaTile label="Caseback" /><button className="grid aspect-square place-items-center rounded-xl border border-dashed border-paper-200 text-ink-500 hover:border-copper-400 hover:text-copper-600"><ImagePlus size={22} /><span className="sr-only">Add image</span></button></div><p className="mt-4 mb-0 text-[11px] text-ink-500">3 of 20 images · all uploads READY and owned by this seller</p></div></div>;
}

function MediaTile({ label, cover }: { label: string; cover?: boolean }) {
  return <div className="group relative aspect-square overflow-hidden rounded-xl bg-[radial-gradient(circle_at_55%_35%,#807b72,#2b2a28_65%,#171716)]"><GripVertical className="absolute top-2 right-2 text-white/65" size={15} /><div className="absolute inset-0 grid place-items-center text-white/40"><Gavel size={38} strokeWidth={1} /></div><span className="absolute bottom-2 left-2 text-[10px] font-bold text-white">{label}</span>{cover && <span className="absolute top-2 left-2 rounded-full bg-copper-500 px-2 py-0.5 text-[9px] font-bold uppercase text-white">Cover</span>}</div>;
}

function DetailsStep({ title, setTitle }: { title: string; setTitle: (value: string) => void }) {
  return <div><div className="border-b border-paper-200 p-5"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Step 2</p><h1 className="mt-1 mb-1 text-2xl font-semibold tracking-[-0.03em]">Describe the lot</h1><p className="m-0 text-xs text-ink-500">Description freezes after the first accepted bid.</p></div><div className="grid gap-5 p-5 sm:grid-cols-2"><Field label="Title" className="sm:col-span-2"><input value={title} onChange={(event) => setTitle(event.target.value)} /></Field><Field label="Category"><select defaultValue="watches"><option value="watches">Watches</option><option value="art">Art</option><option value="design">Design</option></select></Field><Field label="Condition"><select defaultValue="very-good"><option value="very-good">Very good</option><option value="excellent">Excellent</option><option value="fair">Fair</option></select></Field><Field label="Description" className="sm:col-span-2"><textarea rows={6} defaultValue="Calibre 861, original stepped dial, recently serviced. Includes period-correct bracelet and archive extract." /></Field><Field label="Location"><input defaultValue="Berlin, Germany" /></Field><Field label="Seller reference"><input defaultValue="WATCH-1969-042" /></Field></div></div>;
}

function PricingStep() {
  return <div><div className="border-b border-paper-200 p-5"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Step 3</p><h1 className="mt-1 mb-1 text-2xl font-semibold tracking-[-0.03em]">Pricing & timing</h1><p className="m-0 text-xs text-ink-500">Configure the opening price and anti-sniping behavior.</p></div><div className="grid gap-4 border-b border-paper-200 p-5 sm:grid-cols-2"><Field label="Starting price"><input type="number" defaultValue="18000" /></Field><Field label="Bid step"><input type="number" defaultValue="200" /></Field></div><div className="p-5"><ClosingPolicyBuilder initialValue={{ scheduledEnd: '2026-10-24T18:00', inactivityMinutes: 5, hardDeadline: '2026-10-24T20:00' }} /></div></div>;
}

function ReviewStep({ title }: { title: string }) {
  const checks = ['3 images ready and ordered', 'Required lot details complete', 'Starting price €18,000 · step €200', 'Closing policy has a hard deadline', 'Seller permission verified'];
  return <div><div className="border-b border-paper-200 p-5"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-copper-600">Step 4</p><h1 className="mt-1 mb-1 text-2xl font-semibold tracking-[-0.03em]">Ready to publish</h1><p className="m-0 text-xs text-ink-500">Review the immutable auction inputs before listing.</p></div><div className="p-5"><div className="rounded-xl border border-paper-200 bg-paper-100 p-4"><div className="flex items-start gap-3"><Sparkles className="mt-0.5 text-copper-500" size={18} /><div><h2 className="m-0 text-sm font-semibold">{title}</h2><p className="mt-1 mb-0 text-xs text-ink-500">Watches · Berlin, Germany · No reserve</p></div></div></div><ul className="mt-5 mb-0 grid list-none gap-3 p-0 sm:grid-cols-2">{checks.map((check) => <li key={check} className="flex items-center gap-2 text-xs"><span className="grid size-5 place-items-center rounded-full bg-moss-500 text-white"><Check size={11} /></span>{check}</li>)}</ul><div className="mt-6 rounded-xl border border-paper-200 p-4 text-xs leading-5 text-ink-700"><strong>Event-driven opening:</strong> publishing commits the lot and `LotListed` together. Auction opens asynchronously and idempotently.</div></div></div>;
}

function LotPreview({ title, step }: { title: string; step: number }) {
  return <aside className="h-fit rounded-2xl border border-paper-200 bg-paper-50 p-4 shadow-panel lg:sticky lg:top-5"><div className="flex items-center justify-between"><p className="m-0 text-[10px] font-bold uppercase tracking-[0.15em] text-ink-500">Buyer preview</p><StatusBadge tone="neutral">Draft</StatusBadge></div><div className="mt-4 aspect-[4/3] rounded-xl bg-[radial-gradient(circle_at_55%_35%,#807b72,#2b2a28_65%,#171716)]"><div className="grid h-full place-items-center text-white/35"><Gavel size={48} strokeWidth={1} /></div></div><p className="mt-4 mb-1 text-[10px] font-bold uppercase tracking-[0.13em] text-copper-600">Watches</p><h2 className="mt-0 mb-2 text-lg font-semibold leading-6 tracking-[-0.025em]">{title || 'Untitled lot'}</h2><p className="m-0 text-xs leading-5 text-ink-500">Calibre 861, original stepped dial, recently serviced…</p><div className="mt-4 flex items-end justify-between border-t border-paper-200 pt-4"><div><p className="m-0 text-[10px] uppercase tracking-wider text-ink-500">Starts at</p><p className="mt-1 mb-0 text-xl font-bold">€18,000</p></div><span className="font-mono text-[10px] text-ink-500">Step {step + 1}/4</span></div></aside>;
}

function Field({ label, children, className }: { label: string; children: ReactElement<{ className?: string }>; className?: string }) {
  return <label className={cn('block text-xs font-bold text-ink-700', className)}>{label}{<span className="mt-2 block [&>*]:w-full [&>*]:rounded-lg [&>*]:border [&>*]:border-paper-200 [&>*]:bg-white [&>*]:px-3 [&>*]:py-2.5 [&>*]:text-sm [&>*]:font-normal [&>*]:outline-none [&>*]:focus:border-copper-500">{children}</span>}</label>;
}
