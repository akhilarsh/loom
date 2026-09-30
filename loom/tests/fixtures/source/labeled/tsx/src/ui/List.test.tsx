import { List } from "./List";

test("renders the list", () => {
  const view = <List items={["a"]} />;
  expect(view).toBeTruthy();
});
