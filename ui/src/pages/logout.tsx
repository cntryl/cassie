import { state } from "@askrjs/askr";
import { task } from "@askrjs/askr/resources";
import { navigate } from "@askrjs/askr/router";
import { Block, Button, PageHeader, Spinner, Text } from "@askrjs/themes/components";

import { AuthBrand } from "@/components/auth/auth-brand";
import { AuthPage } from "@/components/auth/auth-page";
import { createLogoutMutation } from "@/features/auth-actions";
import { clearQueryWorkspace } from "@/features/query/query-tabs";
import { getSession, signOut } from "@/shared/auth";
import { apiErrorMessage } from "@/shared/errors/api";

type LogoutPhase = "pending" | "error";

export default function LogoutPage() {
  const logoutMutation = createLogoutMutation();
  const session = getSession();
  const [phase, setPhase] = state<LogoutPhase>("pending");
  const [error, setError] = state("");
  const currentPhase = phase();
  const errorMessage = error();

  task(() => signOutAndRedirect());

  async function signOutAndRedirect() {
    setPhase("pending");
    setError("");

    try {
      await logoutMutation.execute();
      if (session?.user) {
        clearQueryWorkspace(session.user);
      }

      signOut();
      navigate("/login", { history: "replace" });
    } catch (caught) {
      setError(apiErrorMessage(caught));
      setPhase("error");
    }
  }

  return (
    <AuthPage>
      <Block direction="column" gap="xl">
        <Block direction="column" gap="md">
          <AuthBrand />
          <PageHeader
            title={currentPhase === "error" ? "Sign out failed" : "Signing out"}
            description={
              currentPhase === "error"
                ? "We could not clear your session. You may still be signed in."
                : "Clearing your Cassie Admin session."
            }
          />
        </Block>

        <Block direction="column" align="start" gap="md" aria-live="polite" aria-atomic="true">
          {currentPhase === "pending" ? <Spinner label="Signing out" /> : null}

          {currentPhase === "error" ? (
            <Block direction="column" align="start" gap="md" role="alert">
              <Text tone="danger" size="sm">
                {errorMessage || "We could not clear your session. You may still be signed in."}
              </Text>
              <Button variant="outline" onPress={() => void signOutAndRedirect()}>
                Retry
              </Button>
            </Block>
          ) : null}
        </Block>
      </Block>
    </AuthPage>
  );
}
