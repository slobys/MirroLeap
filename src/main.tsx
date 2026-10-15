import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import App from "./App";
import { I18nProvider } from "./i18n";

const rootElement = document.getElementById("root") as HTMLElement;
const fallback = document.getElementById("startup-fallback");
// React rendering is asynchronous. Only dismiss the static recovery notice
// once the real component tree has committed; merely loading the JS file or
// scheduling a render would incorrectly produce a blank window on failures.
const observer = new MutationObserver(() => {
  if (rootElement.firstElementChild) {
    fallback?.remove();
    observer.disconnect();
  }
});
observer.observe(rootElement, { childList: true });

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </React.StrictMode>,
);
