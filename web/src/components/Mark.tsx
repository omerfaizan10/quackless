/** The site mark: a three-operator plan tree (root, two inputs). */
export default function Mark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 18 18" aria-hidden="true">
      <rect x="5.5" y="1" width="7" height="5" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <rect x="1" y="12" width="6" height="5" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <rect x="11" y="12" width="6" height="5" fill="#1747d1" stroke="#1747d1" strokeWidth="1.4" />
      <path d="M9 6v3H4v3M9 9h5v3" fill="none" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}
