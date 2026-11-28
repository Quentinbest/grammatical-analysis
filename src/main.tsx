import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { SelectionPill } from "./components/SelectionPill/index.ts";

const isSelectionPillView =
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).get("view") === "selection-pill";

if (isSelectionPillView && typeof document !== "undefined") {
  document.documentElement.dataset.windowView = "selection-pill";
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(
    <StrictMode>{isSelectionPillView ? <SelectionPill /> : <App />}</StrictMode>,
  );
}
