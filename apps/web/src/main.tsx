/**
 * Browser entry point: mounts the Myrlet workspace into `#root`.
 *
 * The Admin bearer token is never persisted here or anywhere else — tokens
 * live only in the `useHub` React state for the lifetime of the tab.
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";
import "./styles/tokens.css";
import "./styles/app.css";

const container = document.getElementById("root");

if (!container) {
  throw new Error('Myrlet could not start: the "#root" element is missing.');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
