import { ref } from "vue";

export type ThemeName = "celadon" | "night";

const KEY = "daodian-theme";

function readTheme(): ThemeName {
  try {
    return localStorage.getItem(KEY) === "night" ? "night" : "celadon";
  } catch {
    return "celadon";
  }
}

export const themeName = ref<ThemeName>(readTheme());

export function setTheme(name: ThemeName) {
  themeName.value = name;
  document.documentElement.dataset.theme = name;
  try {
    localStorage.setItem(KEY, name);
  } catch {
    // 隐私模式里记不住主题。
  }
}

setTheme(themeName.value);
