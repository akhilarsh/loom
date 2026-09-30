import { round } from "./utils";

export class Circle {
  constructor(radius) {
    this.radius = radius;
  }

  area() {
    return round(Math.PI * this.radius ** 2);
  }

  describe() {
    return `circle ${this.area()}`;
  }
}

export class Square {
  constructor(side) {
    this.side = side;
  }

  area() {
    return this.side * this.side;
  }
}

export function largest(shapes) {
  let best = shapes[0];
  for (const shape of shapes) {
    const size = shape.area();
    if (size > best.area()) {
      best = shape;
    }
  }
  return best;
}
