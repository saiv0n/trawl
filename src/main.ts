import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

// tarayıcının sağ tık menüsü sadece metin alanlarında açılsın
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .selectable")) e.preventDefault();
});
// klavyeyle açılan pencere canlanmasın, son girdi türünü tut
addEventListener("keydown", () => ((window as any).__lastInputKeyboard = true), true);
addEventListener("pointerdown", () => ((window as any).__lastInputKeyboard = false), true);

export default mount(App, { target: document.getElementById("root")! });
