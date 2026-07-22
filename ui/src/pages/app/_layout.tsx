import { state } from "@askrjs/askr";
import { Link } from "@askrjs/askr/router";
import { DatabaseIcon, LogOutIcon, MenuIcon, MoonIcon, SunIcon } from "@askrjs/lucide";
import {
  Badge,
  Block,
  Brand,
  BrandLabel,
  BrandMark,
  Button,
  Container,
  Grid,
  Inline,
  Text,
} from "@askrjs/themes/components";
import { Header, NavBrand, NavGroup, Navbar, Sidebar } from "@askrjs/themes/components";
import { ThemeToggle } from "@askrjs/themes/theme";

import { clamp } from "@/shared/drag-resize";
import {
  SIDEBAR_WIDTH_MAX_PX,
  SIDEBAR_WIDTH_MIN_PX,
  SidebarResizeHandle,
} from "@/components/shell/sidebar-resize-handle";
import { SidebarPortalHost } from "@/components/shell/sidebar-portal-host";
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

export default function Layout({ children }: { children?: unknown }) {
  const session = getSession();
  const [mobileNavOpen, setMobileNavOpen] = state(false);
  const [sidebarWidth, setSidebarWidth] = state(readPersistedSidebarWidth());
  const isMobileNavOpen = mobileNavOpen();

  let rootEl: HTMLElement | null = null;
  let clearOverrideFrame: number | null = null;

  function setRootEl(node: unknown) {
    rootEl = node instanceof HTMLElement ? node : null;
  }

  function cancelPendingOverrideClear() {
    if (clearOverrideFrame !== null) {
      cancelAnimationFrame(clearOverrideFrame);
      clearOverrideFrame = null;
    }
  }

  // During a live drag, mutate the CSS var directly (imperative, no state
  // commit) so dragging doesn't force a re-render per pointermove. The
  // element's `style` prop below (driven by sidebarWidth()) is what keeps
  // the var correct the rest of the time — on first mount, after the drag
  // commits, and on *any other* unrelated re-render (e.g. toggling the
  // theme), which previously reset the sidebar because the imperative-only
  // value had no declarative backing and got wiped on the next patch.
  function handleSidebarDragMove(px: number) {
    // A new drag starting/continuing supersedes any previous drag's queued
    // cleanup below — without this, a release-then-immediately-regrab within
    // one frame lets the stale rAF fire mid-drag and wipe this drag's
    // just-applied width back to the old committed value.
    cancelPendingOverrideClear();
    rootEl?.style.setProperty("--cassie-sidebar-width", `${px}px`);
  }

  function handleSidebarDragEnd(px: number) {
    setSidebarWidth(px);
    persistSidebarWidth(px);
    // Clear the imperative override once the declarative style (from the
    // committed sidebarWidth()) has taken over, so it doesn't keep masking
    // future declarative updates via inline-style precedence.
    cancelPendingOverrideClear();
    const node = rootEl;
    clearOverrideFrame = requestAnimationFrame(() => {
      clearOverrideFrame = null;
      node?.style.removeProperty("--cassie-sidebar-width");
    });
  }

  function toggleMobileNavigation() {
    setMobileNavOpen(!mobileNavOpen());
  }

  return (
    <Block
      class="cassie-admin-root"
      data-testid="cassie-admin-shell"
      minHeight="screen"
      direction="column"
      style={{ "--cassie-sidebar-width": `${sidebarWidth()}px` }}
      ref={setRootEl}
    >
      <a class="skip-link" href="#main-content">
        Skip to main content
      </a>

      <Header class="cassie-admin-header" sticky>
        <Container size="full" paddingY="0">
          <Navbar class="cassie-admin-navbar" aria-label="Cassie admin">
            <NavBrand>
              <Brand asChild>
                <Link href="/" aria-label="Cassie admin home">
                  <BrandMark aria-hidden="true">
                    <DatabaseIcon size={16} />
                  </BrandMark>
                  <BrandLabel>Cassie Admin</BrandLabel>
                </Link>
              </Brand>
            </NavBrand>

            <NavGroup align="end" aria-label="View controls" role="group">
              <Inline gap="sm" align="center" data-testid="admin-session-context">
                <Badge variant="success">
                  <DatabaseIcon size={14} aria-hidden="true" />
                  Cassie server
                </Badge>
                {session?.user ? (
                  <Text as="span" size="sm" tone="muted">
                    {session.user}
                  </Text>
                ) : null}
              </Inline>
              <ThemeToggle
                aria-label="Toggle color theme"
                variant="ghost"
                size="icon"
                lightIcon={<SunIcon size={16} />}
                darkIcon={<MoonIcon size={16} />}
              />
              <Button asChild variant="ghost" size="icon">
                <Link href="/logout" aria-label="Sign out">
                  <LogOutIcon size={16} aria-hidden="true" />
                </Link>
              </Button>
            </NavGroup>
          </Navbar>
        </Container>
      </Header>

      <Container class="cassie-admin-workspace" size="full" paddingX="0" paddingY="0" grow>
        <Grid
          class="cassie-admin-layout"
          columns={{
            base: 1,
            md: "var(--cassie-sidebar-width, 18rem) 0.25rem minmax(0, 1fr)",
          }}
          gap="0"
          align="stretch"
        >
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
            <Button
              type="button"
              class="cassie-admin-sidebar-toggle"
              variant="outline"
              aria-controls="cassie-admin-sidebar-panel"
              aria-expanded={isMobileNavOpen}
              aria-label="Toggle schema browser"
              onClick={toggleMobileNavigation}
            >
              <MenuIcon size={16} />
              <span>Schema browser</span>
            </Button>

            <div class="cassie-admin-sidebar-panel" id="cassie-admin-sidebar-panel">
              <div class="cassie-admin-sidebar-extra" data-testid="cassie-admin-sidebar-extra">
                <SidebarPortalHost />
              </div>
            </div>
          </Sidebar>

          <SidebarResizeHandle
            initialPx={sidebarWidth()}
            onDragMove={handleSidebarDragMove}
            onDragEnd={handleSidebarDragEnd}
          />

          <div class="cassie-admin-route-surface">{children}</div>
        </Grid>
      </Container>
    </Block>
  );
}
