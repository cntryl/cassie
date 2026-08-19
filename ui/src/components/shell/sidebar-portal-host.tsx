import { defineScope, readScope, state } from "@askrjs/askr";
import { definePortal } from "@askrjs/askr/foundations";
import type { JSXElement } from "@askrjs/askr/jsx-runtime";

const SidebarPortalScope = defineScope(definePortal());

export function SidebarPortalProvider({ children }: { children?: JSXElement }): JSXElement {
  const [portal] = state(definePortal());
  return <SidebarPortalScope value={portal()}>{children}</SidebarPortalScope>;
}

export function SidebarPortalContent({ children }: { children?: JSXElement }): JSX.Element | null {
  return readScope(SidebarPortalScope).render({ children }) as JSX.Element | null;
}

export function SidebarPortalHost(): JSX.Element | null {
  return readScope(SidebarPortalScope)() as JSX.Element | null;
}
