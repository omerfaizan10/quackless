// Public site identity, used by metadata, the sitemap and the footer.
export const SITE_URL = (process.env.NEXT_PUBLIC_SITE_URL || "https://quackless-sql.vercel.app").replace(/\/$/, "");
export const SITE_NAME = "Quackless";
export const SITE_TITLE = "Quackless: a vectorized SQL engine in Rust, checked against DuckDB";
export const SITE_DESCRIPTION =
  "A columnar, vectorized SQL query engine written from scratch in Rust and compiled to WebAssembly. It answers all 22 TPC-H queries exactly like DuckDB at three scale factors, and runs here in the browser.";
export const REPO_URL = "https://github.com/omerfaizan10/quackless";
export const AUTHOR = {
  name: "Omer Faizan",
  url: "https://omerfaizan.com",
  // Set in web/.env.production so the address lives in one place.
  email: process.env.NEXT_PUBLIC_CONTACT_EMAIL || "",
};
