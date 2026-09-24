import { store } from "@rezejs/signals";

export const [profile, setProfile] = store({ count: 0, user: { name: "Ada" } });
