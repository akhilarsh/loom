const sortBy = require("lodash/sortBy");
const { pad } = require("./format");

function register(items) {
  const sorted = sortBy(items, "name");
  return sorted.map((item) => pad(item.name, 8));
}

module.exports = { register };
