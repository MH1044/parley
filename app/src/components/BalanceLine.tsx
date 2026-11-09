/** The publik API balance line ("$3.12 of publik balance"), as the core writes it; nothing without one. */
export default function BalanceLine({ line }: { line: string | null | undefined }) {
  if (!line) return null
  return <p className="balance-line">{line}</p>
}
