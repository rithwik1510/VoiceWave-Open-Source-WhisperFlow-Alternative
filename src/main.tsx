import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Fonts ship inside the app so nothing is fetched at launch and offline looks the same.
// Fraunces keeps its optical-size axis (opsz.css); both are variable, so every weight is covered.
import "@fontsource-variable/fraunces/opsz.css";
import "@fontsource-variable/manrope";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
