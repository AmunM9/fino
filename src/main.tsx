import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/global.css";
import "./components/ui/ui.css";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
  const { installMocks } = await import("./dev/mockIpc");
  installMocks();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
