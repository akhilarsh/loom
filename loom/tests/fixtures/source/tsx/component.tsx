import { Button } from "./Button";
import { Card as Panel } from "./Card";
export * from "./shared";

interface Props {
  label: string;
}

export function App(props: Props) {
  return (
    <div>
      <Button label={props.label} />
      <Panel>
        <ui.Icon />
      </Panel>
    </div>
  );
}

export class Screen {
  run() {
    this.draw();
    helper();
  }

  draw() {}
}

class Other {
  run() {
    this.run();
    registry.run();
  }
}

function helper() {}
