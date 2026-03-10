import "./style.css";

import { startApp } from "./app";
import { APP_SHELL } from "./app_shell";
import { getAppDom } from "./dom";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("app root not found");
}

app.innerHTML = APP_SHELL;

startApp(getAppDom());
