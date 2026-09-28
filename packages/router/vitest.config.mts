import { domProject } from "../../vitest.shared";
import reze from "../vite-plugin/src/index";

export default domProject("@rezejs/router", [reze({ links: "@rezejs/router" })]);
