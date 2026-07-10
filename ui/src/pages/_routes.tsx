import { group, route, registerRoutes } from "@askrjs/askr/router";

import RootLayout from "./_layout";
import { registerAppRoutes } from "./app/_routes";
import LoginPage from "./login";
import LogoutPage from "./logout";
import { resolveRouteAuth } from "@/shared/auth";

export function registerRootRoutes() {
  registerRoutes(
    () => {
      group({ layout: RootLayout }, () => {
        route("/login", LoginPage, { auth: "guest" });
        route("/logout", LogoutPage, { auth: true });
        registerAppRoutes();
      });
    },
    {
      auth: {
        resolve: resolveRouteAuth,
        loginPath: "/login",
        guestRedirectTo: "/",
      },
    },
  );
}

registerRootRoutes();
