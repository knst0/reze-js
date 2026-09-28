// Vendored from krausest/js-framework-benchmark@f2df01a (frameworks/keyed/octane/src/main.js), Apache-2.0.
import { createRoot } from "octane";
import App from "./App.tsrx";

const target = document.getElementById("main");
if (!target) throw new Error("missing #main");

createRoot(target).render(App);
