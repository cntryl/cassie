import { definePortal } from "@askrjs/askr/foundations";
import type { JSXElement } from "@askrjs/askr/jsx-runtime";

export const SidebarPortal = definePortal();

export function SidebarPortalContent({ children }: { children?: JSXElement }): JSX.Element | null {
  return SidebarPortal.render({ children }) as JSX.Element | null;
}

export function SidebarPortalHost(): JSX.Element | null {
  return SidebarPortal() as JSX.Element | null;
}
