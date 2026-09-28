import { domProject } from "../../vitest.shared";
import reze from "../vite-plugin/src/index";

export default domProject("@rezejs/dom", [reze()]);
