import { Task } from "./models";
import { Storage } from "./storage";
import { parseCount as count } from "./util";

export { slugify } from "./util";
export * from "./models";

export function run(input: string): boolean {
  const store = new Storage();
  const task = new Task(count(input), input);
  return store.add(task, input);
}
