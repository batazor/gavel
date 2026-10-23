/* The product's mark: a gavel head and handle drawn as one shape, in the accent. */
export function GavelMark({ size = 24, className }: { size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" className={className}>
      <path d="M4 20.5 12.5 12" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" />
      <path d="M9.2 5.8 12 3l9 9-2.8 2.8z" fill="currentColor" />
      <path d="M9.2 5.8 18.2 14.8" stroke="var(--color-surface)" strokeWidth="1.2" />
      <path d="M3 21h8" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}
