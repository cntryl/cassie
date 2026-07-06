import { group, registerRoutes } from "@askrjs/askr/router";

import RootLayout from "./_layout";
import { registerAppRoutes } from "./app/_routes";

registerRoutes(() => {
  group({ layout: RootLayout }, () => {
    registerAppRoutes();
  });
});
