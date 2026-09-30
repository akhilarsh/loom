use crate::model::Widget;

pub trait Store {
    fn save(&mut self, widget: &Widget) -> bool;
}

pub struct MemStore {
    items: Vec<u32>,
}

pub struct DiskStore {
    path: String,
}

impl MemStore {
    pub fn new() -> Self {
        MemStore { items: Vec::new() }
    }
}

impl Store for MemStore {
    fn save(&mut self, widget: &Widget) -> bool {
        self.items.push(widget.id);
        true
    }
}

impl Store for DiskStore {
    fn save(&mut self, widget: &Widget) -> bool {
        std::fs::write(&self.path, widget.label()).is_ok()
    }
}

pub fn persist(store: &mut dyn Store, widget: &Widget) -> bool {
    store.save(widget)
}
