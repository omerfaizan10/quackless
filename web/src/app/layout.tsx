import type { Metadata, Viewport } from "next";
import "./globals.css";
import Masthead from "@/components/Masthead";
import Footer from "@/components/Footer";
import { AUTHOR, SITE_DESCRIPTION, SITE_NAME, SITE_TITLE, SITE_URL } from "@/lib/site";

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: { default: SITE_TITLE, template: `%s · ${SITE_NAME}` },
  description: SITE_DESCRIPTION,
  authors: [{ name: AUTHOR.name, url: AUTHOR.url }],
  alternates: { canonical: "/" },
  openGraph: { type: "website", siteName: SITE_NAME, title: SITE_TITLE, description: SITE_DESCRIPTION, url: SITE_URL },
  twitter: { card: "summary", title: SITE_TITLE, description: SITE_DESCRIPTION },
};

export const viewport: Viewport = { themeColor: "#fdfdfb" };

const jsonLd = {
  "@context": "https://schema.org",
  "@type": "SoftwareSourceCode",
  name: SITE_NAME,
  description: SITE_DESCRIPTION,
  codeRepository: "https://github.com/omerfaizan10/quackless",
  programmingLanguage: ["Rust", "WebAssembly", "TypeScript"],
  author: { "@type": "Person", name: AUTHOR.name, url: AUTHOR.url },
  url: SITE_URL,
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <Masthead />
        {children}
        <Footer />
        <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }} />
      </body>
    </html>
  );
}
