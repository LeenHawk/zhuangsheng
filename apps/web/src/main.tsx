import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import "@xyflow/react/dist/style.css";

import { App } from "./app";
import "./styles.css";
import { applyUiPreferences, loadUiPreferences } from "./ui-preferences";

applyUiPreferences(loadUiPreferences());

const root = document.getElementById("root");
if (!root) throw new Error("Application root is missing.");

createRoot(root).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>,
);
