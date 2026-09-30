pub struct Holder<T> {
    value: T,
}

impl<T: Clone> Holder<T> {
    pub fn new(value: T) -> Self {
        Holder { value }
    }

    pub fn get(&self) -> T {
        self.check();
        self.value.clone()
    }

    fn check(&self) {}
}

pub fn held() -> u32 {
    let holder = Holder::new(7u32);
    holder.get()
}
