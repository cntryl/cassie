import { state } from "@askrjs/askr";
import { For } from "@askrjs/askr/control";
import { currentRoute, Link } from "@askrjs/askr/router";
import { MenuIcon, MoonIcon, SunIcon } from "@askrjs/lucide";
import { Block, Brand, BrandLabel, Button, Container, Grid, Text } from "@askrjs/themes/components";
import {
  Header,
  NavBrand,
  NavGroup,
  Navbar,
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@askrjs/themes/components";
import { ThemeToggle } from "@askrjs/themes/theme";

import { adminRoutes } from "@/shared/admin-routes";

function currentPath() {
  try {
    return currentRoute().path;
  } catch {
    // Fall back only for non-router render contexts such as isolated shell tests.
  }

  if (typeof window !== "undefined") {
    return window.location.pathname;
  }

  return "/admin";
}

export default function Layout({ children }: { children?: unknown }) {
  const [mobileNavOpen, setMobileNavOpen] = state(false);
  const path = currentPath();
  const isMobileNavOpen = mobileNavOpen();

  function closeMobileNavigation() {
    setMobileNavOpen(false);
  }

  function toggleMobileNavigation() {
    setMobileNavOpen(!mobileNavOpen());
  }

  function isActive(href: string) {
    return path === href;
  }

  return (
    <Block
      class="cassie-admin-root"
      data-testid="cassie-admin-shell"
      minHeight="screen"
      direction="column"
    >
      <a class="skip-link" href="#main-content">
        Skip to main content
      </a>

      <Header sticky>
        <Container paddingY="sm">
          <Navbar class="cassie-admin-navbar" aria-label="Cassie admin">
            <NavBrand>
              <Brand asChild>
                <Link href="/admin" aria-label="Cassie admin home">
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

      <Container class="cassie-admin-workspace" paddingY="0" grow>
        <Grid
          class="cassie-admin-layout"
          columns={{ base: 1, md: "13rem minmax(0, 1fr)" }}
          gap="md"
          align="start"
        >
          <Sidebar
            class="cassie-admin-sidebar"
            collapsible="none"
            minHeight="auto"
            padding="md"
            borderRight={false}
            shrink={false}
            width="full"
            data-mobile-open={isMobileNavOpen ? "true" : undefined}
            aria-label="Admin navigation"
            role="navigation"
          >
            <Button
              type="button"
              class="cassie-admin-sidebar-toggle"
              variant="outline"
              aria-controls="cassie-admin-sidebar-panel"
              aria-expanded={isMobileNavOpen}
              aria-label="Navigation menu"
              onClick={toggleMobileNavigation}
            >
              <MenuIcon size={16} />
              <span>Navigation</span>
            </Button>

            <div class="cassie-admin-sidebar-panel" id="cassie-admin-sidebar-panel">
              <SidebarContent>
                <SidebarGroup>
                  <SidebarGroupLabel>Workspace</SidebarGroupLabel>
                  <SidebarGroupContent>
                    <SidebarMenu>
                      <For each={adminRoutes} by={(adminRoute) => adminRoute.path}>
                        {(adminRoute) => (
                          <SidebarMenuItem>
                            <SidebarMenuButton active={isActive(adminRoute.path)} asChild>
                              <Link href={adminRoute.path} onClick={closeMobileNavigation}>
                                <adminRoute.icon size={16} aria-hidden="true" />
                                <Text as="span" size="sm" weight="medium">
                                  {adminRoute.label}
                                </Text>
                              </Link>
                            </SidebarMenuButton>
                          </SidebarMenuItem>
                        )}
                      </For>
                    </SidebarMenu>
                  </SidebarGroupContent>
                </SidebarGroup>
              </SidebarContent>
            </div>
          </Sidebar>

          <div class="cassie-admin-route-surface">{children}</div>
        </Grid>
      </Container>
    </Block>
  );
}
