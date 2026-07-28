import { state } from "@askrjs/askr";
import { task } from "@askrjs/askr/resources";
import { navigate } from "@askrjs/askr/router";
import {
  Block,
  Brand,
  BrandLabel,
  BrandMark,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Spinner,
  Text,
} from "@askrjs/themes/components";

import { apiv1 } from "@/adapters";
import cassieLogo from "@/assets/cassie-logo.png";
import { clearQueryWorkspace } from "@/features/query/query-tabs";
import { getSession, signOut } from "@/shared/auth";
import { apiErrorMessage, ensureResponseOk } from "@/shared/errors/api";

type LogoutPhase = "pending" | "error";

export default function LogoutPage() {
  const session = getSession();
  const [phase, setPhase] = state<LogoutPhase>("pending");
  const [error, setError] = state("");
  const currentPhase = phase();
  const errorMessage = error();

  task(() => signOutAndRedirect());

  async function performSignOut() {
    const response = await apiv1.logoutRestSession();
    if (response.ok) {
      return;
    }

    if (response.status === 401) {
      return;
    }

    ensureResponseOk(response, "Unable to sign out");
  }

  async function signOutAndRedirect() {
    setPhase("pending");
    setError("");

    try {
      await performSignOut();
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
    <Block as="main" class="cassie-login-page" background="canvas">
      <Block class="cassie-login-panel" width="full" gap="lg">
        <Card class="cassie-login-card" variant="raised">
          <CardHeader>
            <Brand>
              <BrandMark aria-hidden="true">
                <img class="cassie-brand-logo" src={cassieLogo} alt="" />
              </BrandMark>
              <BrandLabel>Cassie Admin</BrandLabel>
            </Brand>
            <CardTitle titleAs="h1">
              {currentPhase === "error" ? "Sign out failed" : "Signing out"}
            </CardTitle>
            <CardDescription>
              {currentPhase === "error"
                ? "We could not clear your session. You may still be signed in."
                : "Clearing your Cassie Admin session."}
            </CardDescription>
          </CardHeader>
          <CardContent>
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
          </CardContent>
        </Card>
      </Block>
    </Block>
  );
}
