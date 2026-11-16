import { AUTHOR, REPO_URL } from "@/lib/site";

export default function Footer() {
  return (
    <footer className="colophon">
      <div className="wrap">
        <p style={{ margin: 0, maxWidth: "70ch" }}>
          Built by{" "}
          <a href={AUTHOR.url} rel="author">
            {AUTHOR.name}
          </a>
          . Source and every number on this site:{" "}
          <a href={REPO_URL}>github.com/omerfaizan10/quackless</a> (run <code>make eval</code>)
          {AUTHOR.email && (
            <>
              . Contact: <a href={`mailto:${AUTHOR.email}`}>{AUTHOR.email}</a>
            </>
          )}
          .
        </p>
        <p className="tiny" style={{ margin: 0 }}>
          TPC-H data generated with DuckDB&apos;s tpch extension. sqllogictest files from the SQLite project
          (public domain).
        </p>
      </div>
    </footer>
  );
}
