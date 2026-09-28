// Vendored from krausest/js-framework-benchmark@f2df01a (frameworks/keyed/svelte/src/main.js), Apache-2.0.
import { mount } from "svelte";
import Main from "./Main.svelte";

mount(Main, {
  target: document.querySelector("#main"),
});
