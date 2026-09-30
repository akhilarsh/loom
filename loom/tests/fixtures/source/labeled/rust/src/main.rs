mod model;
mod store;
mod util;

#[cfg(test)]
mod tests;

use crate::model::{Count, Widget};
use crate::store::{persist, MemStore};
use crate::util::normalize as canonical;
use crate::util::parse_id;
use serde_json::to_string;

fn main() {
    let id = parse_id("7").unwrap_or(0);
    let widget = Widget::new(id, "  Bolt ");
    let key = canonical(&widget.name);
    let mut mem = MemStore::new();
    let saved = persist(&mut mem, &widget);
    let total = Count::from(3u8);
    let json = to_string(&widget.label());
    println!("{key} {saved} {} {:?}", total.value, json);
}
