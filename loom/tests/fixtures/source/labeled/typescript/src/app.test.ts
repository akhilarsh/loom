import { run } from "./index";
import { headline } from "./util";

test("run accepts a number", () => {
  expect(run("3")).toBe(true);
});

test("headline keeps the text", () => {
  expect(headline("a")).toBe("a");
});
