import { Circle, largest } from "../src/shapes";

test("largest returns the bigger shape", () => {
  const big = new Circle(2);
  expect(largest([big])).toBe(big);
});
