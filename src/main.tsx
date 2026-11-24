import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./index.css";
import { applyCachedTheme } from "./lib/theme";
import { applyCachedZoom } from "./lib/zoom";

applyCachedTheme();
applyCachedZoom();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
