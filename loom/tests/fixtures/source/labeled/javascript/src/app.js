import { biggest, percent } from "./index";
import { Circle, Square } from "./shapes";
import { register } from "./registry";

export function report(radius, side) {
  const shapes = [new Circle(radius), new Square(side)];
  const winner = biggest(shapes);
  const names = register([{ name: "circle" }, { name: "square" }]);
  return { area: percent(winner.area()), names };
}
