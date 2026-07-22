import { createSPA } from "@askrjs/askr/boot";
import { getManifest } from "@askrjs/askr/router";

import "./shared/monaco-workers";
import "./pages/_routes";

void createSPA({
  root: document.getElementById("app")!,
  manifest: getManifest(),
});
