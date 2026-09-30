import { run, slugify } from "./index";

export function main(argv: string[]): number {
  const name = slugify(argv[0] ?? "");
  return run(name) ? 0 : 1;
}
