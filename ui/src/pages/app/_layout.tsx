import { state } from "@askrjs/askr";
import { DefaultPortal } from "@askrjs/askr/foundations";
import { Link } from "@askrjs/askr/router";
import { MenuIcon, MoonIcon, SunIcon } from "@askrjs/lucide";
import { Block, Brand, BrandLabel, Button, Container, Grid } from "@askrjs/themes/components";
import { Header, NavBrand, NavGroup, Navbar, Sidebar } from "@askrjs/themes/components";
import { ThemeToggle } from "@askrjs/themes/theme";

import { clamp } from "@/components/query/resizable-split";
import {
  SIDEBAR_WIDTH_MAX_PX,
  SIDEBAR_WIDTH_MIN_PX,
  SidebarResizeHandle,
} from "@/components/shell/sidebar-resize-handle";

const SIDEBAR_WIDTH_STORAGE_KEY = "cassie-admin-sidebar-width";
const SIDEBAR_WIDTH_DEFAULT_PX = 288;

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

function SidebarPortalHost(): JSX.Element | null {
  return DefaultPortal() as JSX.Element | null;
}

export default function Layout({ children }: { children?: unknown }) {
  const [mobileNavOpen, setMobileNavOpen] = state(false);
  const [sidebarWidth, setSidebarWidth] = state(readPersistedSidebarWidth());
  const isMobileNavOpen = mobileNavOpen();

  let rootEl: HTMLElement | null = null;

  function setRootEl(node: unknown) {
    rootEl = node instanceof HTMLElement ? node : null;
    if (rootEl) {
      rootEl.style.setProperty("--cassie-sidebar-width", `${sidebarWidth()}px`);
    }
  }

  function handleSidebarDragMove(px: number) {
    rootEl?.style.setProperty("--cassie-sidebar-width", `${px}px`);
  }

  function handleSidebarDragEnd(px: number) {
    setSidebarWidth(px);
    persistSidebarWidth(px);
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
      ref={setRootEl}
    >
      <a class="skip-link" href="#main-content">
        Skip to main content
      </a>

      <Header sticky>
        <Container size="full" paddingY="sm">
          <Navbar class="cassie-admin-navbar" aria-label="Cassie admin">
            <NavBrand>
              <Brand asChild>
                <Link href="/" aria-label="Cassie admin home">
                  <BrandLabel>Cassie Admin</BrandLabel>
                </Link>
              </Brand>
            </NavBrand>

            <NavGroup align="end" aria-label="View controls" role="group">
              <ThemeToggle
                aria-label="Toggle color theme"
                variant="ghost"
                size="icon"
                lightIcon={<SunIcon size={16} />}
                darkIcon={<MoonIcon size={16} />}
              />
            </NavGroup>
          </Navbar>
        </Container>
      </Header>

      <Container class="cassie-admin-workspace" size="full" paddingX="0" paddingY="0" grow>
        <Grid
          class="cassie-admin-layout"
          columns={{
            base: 1,
            md: "var(--cassie-sidebar-width, 18rem) var(--ak-space-2, 0.5rem) minmax(0, 1fr)",
          }}
          gap="0"
          align="stretch"
        >
          <Sidebar
            class="cassie-admin-sidebar"
            collapsible="none"
            minHeight="auto"
            padding="md"
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
