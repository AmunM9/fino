/** Fino's mark: a single slender leaf — light, precise, nothing extra. */
export function LogoMark({ size = 28 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" aria-hidden="true">
      <g transform="rotate(38 16 16)">
        <path d="M16 3.5C22.6 9.4 22.6 22.6 16 28.5 9.4 22.6 9.4 9.4 16 3.5Z" fill="var(--color-signal)" />
        <path d="M16 7.5V26" stroke="var(--color-bg)" strokeWidth="1.6" strokeLinecap="round" />
      </g>
    </svg>
  );
}
