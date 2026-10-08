import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ipc } from "./lib/ipc";
import { setLanguage } from "./lib/strings";
import "./styles/global.css";
import "./components/ui/ui.css";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMocks } = await import("./dev/mockIpc");
  installMocks();
}

// The first render is already in the right language: no flash of the other one.
setLanguage(await ipc.uiLanguage().catch(() => (navigator.language.startsWith("es") ? "es" : "en")));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
