import { state } from "@askrjs/askr";
import { ErrorBoundary } from "@askrjs/askr/components";
import { task } from "@askrjs/askr/resources";
import { Link } from "@askrjs/askr/router";
import { LogOutIcon, MenuIcon, MoonIcon, SunIcon } from "@askrjs/lucide";
import {
  Brand,
  BrandLabel,
  BrandMark,
  Alert,
  Button,
  EmptyState,
  Inline,
  Shell,
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarInset,
  SidebarScope,
  Text,
} from "@askrjs/themes/components";
import { ThemeScope, ThemeToggle } from "@askrjs/themes/theme";

import { clamp } from "@/shared/drag-resize";
import { cassieLogoImageProps, cassieLogoPath } from "@/shared/cassie-brand-assets";
import {
  SIDEBAR_WIDTH_MAX_PX,
  SIDEBAR_WIDTH_MIN_PX,
  SidebarResizeHandle,
} from "@/components/shell/sidebar-resize-handle";
import { SidebarPortalHost, SidebarPortalProvider } from "@/components/shell/sidebar-portal-host";
import { getSession } from "@/shared/auth";

const SIDEBAR_WIDTH_STORAGE_KEY = "cassie-admin-sidebar-width";
const SIDEBAR_WIDTH_DEFAULT_PX = 280;

function readPersistedSidebarWidth(): number {
  if (typeof window === "undefined") {
    return SIDEBAR_WIDTH_DEFAULT_PX;
  }

  try {
    const stored = window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const parsed = stored === null ? Number.NaN : Number.parseFloat(stored);
    if (Number.isNaN(parsed)) {
      return SIDEBAR_WIDTH_DEFAULT_PX;
    }

    return clamp(parsed, SIDEBAR_WIDTH_MIN_PX, SIDEBAR_WIDTH_MAX_PX);
  } catch {
    return SIDEBAR_WIDTH_DEFAULT_PX;
  }
}

function persistSidebarWidth(px: number) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(px));
  } catch {
    // Ignore persistence failures (private browsing, storage disabled, etc.).
  }
}

export default function Layout(props: { children?: unknown }) {
  return (
    <ErrorBoundary
      fallback={(_error, reset) => (
        <main id="main-content" tabindex={-1}>
          <EmptyState
            title="Admin workspace unavailable"
            titleAs="h1"
            description="The protected workspace encountered an unexpected problem. Your saved query drafts were not removed."
            action={
              <Button type="button" variant="primary" onPress={reset}>
                Retry workspace
              </Button>
            }
          >
            <Alert
              variant="danger"
              title="Workspace error"
              description="Retry the workspace or sign out and start a new session."
            />
          </EmptyState>
        </main>
      )}
    >
      <SidebarPortalProvider>
        <ProtectedShell {...props} />
      </SidebarPortalProvider>
    </ErrorBoundary>
  );
}

function ProtectedShell({ children }: { children?: unknown }) {
  const session = getSession();
  const [mobileNavOpen, setMobileNavOpen] = state(false);
  const [sidebarWidth, setSidebarWidth] = state(readPersistedSidebarWidth());
  const [elements] = state<{
    root: HTMLElement | null;
    clearOverrideFrame: number | null;
  }>({ root: null, clearOverrideFrame: null });
  const isMobileNavOpen = mobileNavOpen();

  function cancelPendingOverrideClear() {
    const frame = elements().clearOverrideFrame;
    if (frame !== null) {
      cancelAnimationFrame(frame);
      elements().clearOverrideFrame = null;
    }
  }

  task(() => () => cancelPendingOverrideClear());

  function handleSidebarDragMove(px: number) {
    cancelPendingOverrideClear();
    elements().root?.style.setProperty("--cassie-sidebar-width", `${px}px`);
  }

  function handleSidebarDragEnd(px: number) {
    setSidebarWidth(px);
    persistSidebarWidth(px);
    cancelPendingOverrideClear();
    const node = elements().root;
    elements().clearOverrideFrame = requestAnimationFrame(() => {
      elements().clearOverrideFrame = null;
      node?.style.removeProperty("--cassie-sidebar-width");
    });
  }

  function toggleMobileNavigation() {
    setMobileNavOpen(!mobileNavOpen());
  }

  return (
    <Shell
      class="cassie-admin-root"
      data-testid="cassie-admin-shell"
      minHeight="screen"
      direction="column"
      style={{ "--cassie-sidebar-width": `${sidebarWidth()}px` }}
      ref={(node: unknown) => {
        elements().root = node instanceof HTMLElement ? node : null;
      }}
    >
      <a class="skip-link" href="#main-content">
        Skip to main content
      </a>

      <SidebarScope class="cassie-admin-workspace cassie-admin-layout">
        <Sidebar
          class="cassie-admin-sidebar"
          collapsible="none"
          minHeight="auto"
          padding="sm"
          gap="0"
          borderRight
          shrink={false}
          width="full"
          data-mobile-open={isMobileNavOpen ? "true" : undefined}
          aria-label="Schema browser"
        >
          <SidebarHeader class="cassie-admin-sidebar-brand">
            <Brand asChild>
              <Link href="/" aria-label="Cassie admin home">
                <BrandMark class="cassie-brand-mark" aria-hidden="true">
                  <img
                    data-testid="cassie-brand-logo"
                    src={cassieLogoPath}
                    {...cassieLogoImageProps}
                    width="32"
                    height="32"
                    alt=""
                  />
                </BrandMark>
                <BrandLabel>Cassie Admin</BrandLabel>
              </Link>
            </Brand>

            <Button
              type="button"
              class="cassie-admin-sidebar-toggle"
              variant="outline"
              aria-controls="cassie-admin-sidebar-panel"
              aria-expanded={isMobileNavOpen ? "true" : "false"}
              aria-label="Toggle schema browser"
              onPress={toggleMobileNavigation}
            >
              <MenuIcon size={16} />
              <span>Schema browser</span>
            </Button>
          </SidebarHeader>

          <SidebarContent class="cassie-admin-sidebar-panel" id="cassie-admin-sidebar-panel">
            <div class="cassie-admin-sidebar-extra" data-testid="cassie-admin-sidebar-extra">
              <SidebarPortalHost />
            </div>
          </SidebarContent>

          <SidebarFooter class="cassie-admin-sidebar-footer" data-testid="admin-sidebar-footer">
            <Inline gap="sm" align="center" data-testid="admin-session-context">
              {session?.user ? (
                <Text as="span" size="sm" title={session.user} class="cassie-user-name">
                  {session.user}
                </Text>
              ) : null}
            </Inline>
            <ThemeScope defaultTheme="system" storageKey="cassie-admin-theme">
              <ThemeToggle
                aria-label="Toggle color theme"
                variant="ghost"
                size="icon"
                lightIcon={<SunIcon size={16} />}
                darkIcon={<MoonIcon size={16} />}
              />
            </ThemeScope>
            <Button asChild variant="ghost" size="icon">
              <Link href="/logout" aria-label="Sign out">
                <LogOutIcon size={16} aria-hidden="true" />
              </Link>
            </Button>
          </SidebarFooter>
        </Sidebar>

        <div role="navigation" aria-label="Sidebar resizing">
          <SidebarResizeHandle
            initialPx={sidebarWidth()}
            onDragMove={handleSidebarDragMove}
            onDragEnd={handleSidebarDragEnd}
          />
        </div>

        <SidebarInset as="div" class="cassie-admin-route-surface">
          {children as never}
        </SidebarInset>
      </SidebarScope>
    </Shell>
  );
}
