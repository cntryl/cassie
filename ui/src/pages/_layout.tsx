import "../styles/index.css";
import { ThemeScope } from "@askrjs/themes/theme";

export default function RootLayout({ children }: { children?: unknown }) {
  return (
    <ThemeScope defaultTheme="system" storageKey="cassie-admin-theme">
      {children}
    </ThemeScope>
  );
}
