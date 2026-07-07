import { afterEach, describe, expect, it } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import AdminPlaceholderPage from "@/pages/app/placeholder";
import { adminRoutes } from "@/shared/admin-routes";

async function mountRoute(path: string) {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", path);

  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      {
        path,
        handler: () => (
          <RootLayout>
            <AppLayout>
              <AdminPlaceholderPage />
            </AppLayout>
          </RootLayout>
        ),
      },
    ],
  });

  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  return root;
}

afterEach(() => {
  cleanupApp("app");
  document.body.innerHTML = "";
});

describe("admin shell smoke", () => {
  it("should_render_the_cassie_shell_once_for_each_scaffold_route", async () => {
    for (const adminRoute of adminRoutes) {
      const root = await mountRoute(adminRoute.path);

      expect(root.querySelectorAll('[data-testid="cassie-admin-shell"]')).toHaveLength(1);
      expect(root.querySelector("header")).toBeTruthy();
      expect(root.querySelector('[aria-label="Admin navigation"][role="navigation"]')).toBeTruthy();
      expect(root.querySelectorAll("main#main-content")).toHaveLength(1);
      expect(root.textContent).toContain("Cassie Admin");
      expect(root.textContent).toContain(adminRoute.label);
      expect(root.querySelector(`a[href="${adminRoute.path}"][data-active="true"]`)).toBeTruthy();

      cleanupApp("app");
      document.body.innerHTML = "";
    }
  });
});
