import react from "@vitejs/plugin-react";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [react()],
};
