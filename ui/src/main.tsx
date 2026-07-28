import { createSPA } from "@askrjs/askr/boot";

import "./shared/monaco-workers";
import { routeRegistry } from "./pages/_routes";

void createSPA({
  root: document.getElementById("app")!,
  registry: routeRegistry,
});
