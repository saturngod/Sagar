import React from "react";
import ReactDOM from "react-dom/client";
import App from "./app/App";
import { AiBatchProvider } from "./features/translator/aiBatch";
import { UpdateProvider } from "./features/updates/UpdateProvider";
import { LanguageProvider } from "./lib/i18n";
import "./styles/tokens.css";
import "./styles/app.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LanguageProvider>
      <UpdateProvider>
        <AiBatchProvider>
          <App />
        </AiBatchProvider>
      </UpdateProvider>
    </LanguageProvider>
  </React.StrictMode>,
);
