import type { Metadata } from "next";
import { SidebarLayout } from "@/components/SidebarLayout";
import "./globals.css";

export const metadata: Metadata = {
  title: "Centinel",
  description: "Civic transparency viewer + control panel",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className="h-full antialiased">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="anonymous" />
        <link
          href="https://fonts.googleapis.com/css2?family=IM+Fell+English+SC&family=Libre+Baskerville:ital,wght@0,400;0,700;1,400&family=Playfair+Display:ital,wght@0,400;0,600;0,700;0,900;1,400&display=swap"
          rel="stylesheet"
        />
      </head>
      <body className="min-h-full">
        <SidebarLayout>{children}</SidebarLayout>
      </body>
    </html>
  );
}
