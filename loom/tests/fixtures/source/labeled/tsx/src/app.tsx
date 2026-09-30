import { createRoot } from "react-dom/client";
import { List } from "./ui/List";
import { Sidebar } from "./ui/Panel";

export function mount(root: HTMLElement) {
  createRoot(root).render(
    <div>
      <Sidebar title="menu" />
      <List items={["a", "b"]} />
    </div>,
  );
}
