import "../styles/index.css";
import { ErrorBoundary } from "@askrjs/askr/components";
import { Alert, Button, EmptyState } from "@askrjs/themes/components";
import { ThemeScope } from "@askrjs/themes/theme";

export default function RootLayout({ children }: { children?: unknown }) {
  return (
    <ErrorBoundary
      fallback={(_error, reset) => (
        <ThemeScope defaultTheme="system" storageKey="cassie-admin-theme">
          <main id="main-content" tabindex={-1}>
            <EmptyState
              title="Cassie Admin could not open"
              titleAs="h1"
              description="The page encountered an unexpected problem. Your saved query drafts were not removed."
              action={
                <Button type="button" variant="primary" onPress={reset}>
                  Try again
                </Button>
              }
            >
              <Alert
                variant="danger"
                title="Page unavailable"
                description="Retry the page. If the problem continues, check the Cassie service logs."
              />
            </EmptyState>
          </main>
        </ThemeScope>
      )}
    >
      <>
        <ThemeCoordinator />
        {children as never}
      </>
    </ErrorBoundary>
  );
}

function ThemeCoordinator() {
  return (
    <ThemeScope defaultTheme="system" storageKey="cassie-admin-theme">
      <span hidden aria-hidden="true" data-cassie-theme-coordinator="true" />
    </ThemeScope>
  );
}
