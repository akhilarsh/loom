const util = require("./util");
const { parse, format: fmt = String, ...others } = require("./codec");
require("./polyfill");
const importer = require("./importer");

function run() {
  util.parse();
  parse();
}
