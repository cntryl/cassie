import { currentRoute } from "@askrjs/askr/router";
import { Text } from "@askrjs/themes/components";

import { adminRouteForPath } from "@/shared/admin-routes";

function activePath() {
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

export default function AdminPlaceholderPage() {
  const adminRoute = adminRouteForPath(activePath());
  const Icon = adminRoute.icon;

  return (
    <main
      class="cassie-admin-page"
      data-slot="main"
      id="main-content"
      tabindex={-1}
      aria-labelledby="cassie-admin-page-title"
    >
      <section class="cassie-admin-page-header" aria-label={adminRoute.label}>
        <div class="cassie-admin-page-icon" aria-hidden="true">
          <Icon size={20} />
        </div>
        <div class="cassie-admin-page-title-group">
          <Text
            as="p"
            class="cassie-admin-page-kicker"
            size="sm"
            weight="semibold"
            transform="uppercase"
          >
            Cassie
          </Text>
          <h1 id="cassie-admin-page-title">{adminRoute.label}</h1>
          <p>{adminRoute.description}</p>
        </div>
      </section>

      <section class="cassie-admin-placeholder" aria-label={`${adminRoute.label} surface`}>
        <Text as="p" size="sm" weight="medium">
          Admin shell scaffold
        </Text>
      </section>
    </main>
  );
}
