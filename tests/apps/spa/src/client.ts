import { render } from "reze-js";

void render().then(() => {
  document.documentElement.dataset.mounted = location.pathname;
});
