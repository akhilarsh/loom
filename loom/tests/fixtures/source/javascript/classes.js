import { a as b } from "./alias";
export * from "./everything";
export { c as d } from "./other";

function run() {
  return 1;
}

class Runner {
  start() {
    this.run();
    run();
    b();
  }

  run() {
    return this.helper();
  }

  helper() {}
}

class Worker {
  run() {
    obj.run();
  }
}

export const LIMIT = 10;
export const handler = async (event) => run();
export const config = { onLoad: () => 1 };
var legacy = function () {};
