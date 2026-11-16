"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import Mark from "./Mark";

const NAV = [
  { href: "/", label: "Workbench" },
  { href: "/sql/", label: "SQL" },
  { href: "/conformance/", label: "Conformance" },
  { href: "/fuzzer/", label: "Fuzzer" },
  { href: "/internals/", label: "Internals" },
];

export default function Masthead() {
  const path = usePathname() || "/";
  const norm = path.endsWith("/") ? path : path + "/";
  return (
    <header className="masthead">
      <div className="wrap">
        <Link href="/" className="wordmark">
          <Mark />
          Quackless
        </Link>
        <nav aria-label="Main">
          {NAV.map((n) => (
            <Link key={n.href} href={n.href} aria-current={norm === n.href ? "page" : undefined}>
              {n.label}
            </Link>
          ))}
        </nav>
        <span className="status">Rust engine, compiled to WebAssembly</span>
      </div>
    </header>
  );
}
