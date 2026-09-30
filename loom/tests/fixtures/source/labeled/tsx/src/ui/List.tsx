import { Badge } from "./Badge";
import { Panel } from "./Panel";
import { shorten as trim, Bold } from "../text";

export function List({ items }: { items: string[] }) {
  return (
    <Panel title="Items">
      {items.map((item) => (
        <li key={item}>
          <Bold>{trim(item, 12)}</Bold>
          <Badge text="new" />
        </li>
      ))}
    </Panel>
  );
}
