import "../styles/index.css";
import { ThemeProvider } from "@askrjs/themes/theme";

export default function RootLayout({ children }: { children?: unknown }) {
  return (
    <ThemeProvider defaultTheme="system" storageKey="cassie-admin-theme">
      {children}
    </ThemeProvider>
  );
}
