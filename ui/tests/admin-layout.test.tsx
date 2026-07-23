import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import { signIn, signOut } from "@/shared/auth";

const SIDEBAR_WIDTH_STORAGE_KEY = "cassie-admin-sidebar-width";

function testLocalStorage(): Storage {
  try {
    if (window.localStorage) {
      return window.localStorage;
    }
  } catch {
    // Fall through to the in-memory shim below.
  }

  const items = new Map<string, string>();
  const storage = {
    get length() {
      return items.size;
    },
    clear() {
      items.clear();
    },
    getItem(key: string) {
      return items.get(key) ?? null;
    },
    key(index: number) {
      return [...items.keys()][index] ?? null;
    },
    removeItem(key: string) {
      items.delete(key);
    },
    setItem(key: string, value: string) {
      items.set(key, value);
    },
  } satisfies Storage;

  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: storage,
  });

  return storage;
}

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
  signOut();
  cleanupApp("app");
  document.body.innerHTML = "";
});

beforeEach(() => {
  signIn("admin", "password");
  testLocalStorage().removeItem(SIDEBAR_WIDTH_STORAGE_KEY);
});

// jsdom doesn't implement the Pointer Capture API; no-op stubs are enough
// since the handle only calls them, it never relies on capture actually
// taking effect within a test.
function stubPointerCapture(el: HTMLElement) {
  el.setPointerCapture = () => {};
  el.releasePointerCapture = () => {};
}

describe("admin shell sidebar resize", () => {
  it("should_render_full_height_navigation_without_a_top_header", async () => {
    // Arrange
    const root = await mountAdminShell();

    // Act
    const brandLink = root.querySelector('a[aria-label="Cassie admin home"]');
    const brandLogo = brandLink?.querySelector('img[data-testid="cassie-brand-logo"]');
    const sidebar = root.querySelector('[aria-label="Schema browser"]');
    const footer = root.querySelector('[data-testid="admin-sidebar-footer"]');

    // Assert
    expect(brandLink?.textContent).toContain("Cassie Admin");
    expect(brandLogo).toBeInstanceOf(HTMLImageElement);
    expect(brandLogo?.getAttribute("alt")).toBe("");
    expect(brandLogo?.getAttribute("src")).toContain("cassie-logo.png");
    expect(root.querySelector(".cassie-admin-header")).toBeNull();
    expect(sidebar?.contains(brandLink ?? null)).toBe(true);
    expect(sidebar?.contains(footer ?? null)).toBe(true);
    expect(footer?.querySelector('[aria-label="Toggle color theme"]')).not.toBeNull();
    expect(footer?.querySelector('a[aria-label="Sign out"]')).not.toBeNull();
  });

  it("should_surface_active_session_context_given_an_authenticated_admin", async () => {
    // Arrange
    const root = await mountAdminShell();

    // Act
    const sessionContext = root.querySelector('[data-testid="admin-session-context"]');

    // Assert
    expect(sessionContext?.textContent).not.toContain("Cassie server");
    expect(sessionContext?.textContent).toContain("admin");
  });

  it("exposes a keyboard-resizable sidebar handle", async () => {
    const root = await mountAdminShell();

    const handle = root.querySelector('[data-testid="admin-sidebar-resize-handle"]');
    if (!(handle instanceof HTMLElement)) {
      throw new Error("Missing sidebar resize handle");
    }

    expect(handle.getAttribute("role")).toBe("separator");
    expect(handle.getAttribute("aria-orientation")).toBe("vertical");
    expect(handle.getAttribute("aria-valuemin")).toBe("224");
    expect(handle.getAttribute("aria-valuemax")).toBe("512");

    const initialValue = Number(handle.getAttribute("aria-valuenow"));

    handle.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }));
    await flushUi();

    const nextValue = Number(handle.getAttribute("aria-valuenow"));
    expect(nextValue).toBeGreaterThan(initialValue);
    expect(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe(String(nextValue));
  });

  it("keeps the pointer-dragged width after release instead of reverting to the CSS fallback", async () => {
    const root = await mountAdminShell();

    const handle = root.querySelector('[data-testid="admin-sidebar-resize-handle"]');
    const shell = root.querySelector('[data-testid="cassie-admin-shell"]');
    if (!(handle instanceof HTMLElement) || !(shell instanceof HTMLElement)) {
      throw new Error("Missing sidebar resize handle or shell root");
    }
    stubPointerCapture(handle);

    // The steady-state value is driven by a declarative style prop (routed
    // through a generated class, not a literal inline style), while a live
    // drag mutates the same custom property as a genuine inline style —
    // getComputedStyle reflects the cascade correctly either way.
    const initialWidth = getComputedStyle(shell).getPropertyValue("--cassie-sidebar-width");
    expect(initialWidth).not.toBe("");

    handle.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, clientX: 300, pointerId: 1 }),
    );
    await flushUi();

    handle.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, clientX: 360, pointerId: 1 }),
    );
    await flushUi();

    const midWidth = getComputedStyle(shell).getPropertyValue("--cassie-sidebar-width");
    expect(midWidth).not.toBe(initialWidth);

    handle.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, clientX: 360, pointerId: 1 }),
    );
    await flushUi();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    const finalWidth = getComputedStyle(shell).getPropertyValue("--cassie-sidebar-width");
    expect(finalWidth).toBe(midWidth);
    expect(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe(
      String(Number.parseFloat(finalWidth)),
    );
  });

  it("keeps the resized sidebar width after an unrelated re-render of Layout", async () => {
    const root = await mountAdminShell();

    const handle = root.querySelector('[data-testid="admin-sidebar-resize-handle"]');
    const shell = root.querySelector('[data-testid="cassie-admin-shell"]');
    const mobileToggle = root.querySelector('[aria-label="Toggle schema browser"]');
    if (
      !(handle instanceof HTMLElement) ||
      !(shell instanceof HTMLElement) ||
      !(mobileToggle instanceof HTMLElement)
    ) {
      throw new Error("Missing sidebar resize handle, shell root, or mobile nav toggle");
    }

    handle.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }));
    await flushUi();

    const widthAfterResize = getComputedStyle(shell).getPropertyValue("--cassie-sidebar-width");
    expect(widthAfterResize).not.toBe("");

    // Trigger a re-render of Layout that has nothing to do with the sidebar
    // width (mirrors what a theme toggle does — some unrelated bit of state
    // in the same component changes) and confirm the width survives it,
    // rather than reverting to the CSS fallback.
    mobileToggle.click();
    await flushUi();

    const widthAfterUnrelatedRerender =
      getComputedStyle(shell).getPropertyValue("--cassie-sidebar-width");
    expect(widthAfterUnrelatedRerender).toBe(widthAfterResize);
  });
});
