import { fallback, group, route, registerRoutes } from "@askrjs/askr/router";
import { requireAnonymous, requireUser } from "@askrjs/auth";

import RootLayout from "./_layout";
import { registerAppRoutes } from "./app/_routes";
import LoginPage from "./login";
import LogoutPage from "./logout";
import NotFoundPage from "./not-found";
import { resolveRouteAuth } from "@/shared/auth";

export function registerRootRoutes() {
  registerRoutes(
    () => {
      group({ layout: RootLayout }, () => {
        route("/login", LoginPage, { auth: requireAnonymous() });
        route("/logout", LogoutPage, { auth: requireUser() });
        registerAppRoutes();
        fallback(NotFoundPage);
      });
    },
    {
      auth: {
        resolve: resolveRouteAuth,
        loginPath: "/login",
        authenticatedRedirectTo: "/",
      },
    },
  );
}

registerRootRoutes();
