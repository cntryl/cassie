import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";

const SIDEBAR_WIDTH_STORAGE_KEY = "cassie-admin-sidebar-width";

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function mountAdminShell() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", "/");

  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      {
        path: "/",
        handler: () => (
          <RootLayout>
            <AppLayout>
              <div>placeholder route content</div>
            </AppLayout>
          </RootLayout>
        ),
      },
    ],
  });

  await flushUi();
  return root;
}

afterEach(() => {
  cleanupApp("app");
  document.body.innerHTML = "";
});

beforeEach(() => {
  window.localStorage.removeItem(SIDEBAR_WIDTH_STORAGE_KEY);
});

describe("admin shell sidebar resize", () => {
  it("exposes a keyboard-resizable sidebar handle", async () => {
    const root = await mountAdminShell();

    const handle = root.querySelector('[data-testid="admin-sidebar-resize-handle"]');
    if (!(handle instanceof HTMLElement)) {
      throw new Error("Missing sidebar resize handle");
    }

    expect(handle.getAttribute("role")).toBe("separator");
    expect(handle.getAttribute("aria-orientation")).toBe("horizontal");
    expect(handle.getAttribute("aria-valuemin")).toBe("224");
    expect(handle.getAttribute("aria-valuemax")).toBe("512");

    const initialValue = Number(handle.getAttribute("aria-valuenow"));

    handle.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }));
    await flushUi();

    const nextValue = Number(handle.getAttribute("aria-valuenow"));
    expect(nextValue).toBeGreaterThan(initialValue);
    expect(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe(String(nextValue));
  });
});
