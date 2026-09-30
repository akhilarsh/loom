pub const MAX_ID: u32 = 9999;

pub struct Widget {
    pub id: u32,
    pub name: String,
}

pub struct Count {
    pub value: u32,
}

impl Widget {
    pub fn new(id: u32, name: &str) -> Self {
        Widget {
            id: Self::clamp(id),
            name: crate::util::normalize(name),
        }
    }

    pub fn label(&self) -> String {
        self.describe()
    }

    fn describe(&self) -> String {
        format!("{}:{}", self.id, self.name)
    }

    fn clamp(id: u32) -> u32 {
        if id > MAX_ID { MAX_ID } else { id }
    }
}

impl From<u8> for Count {
    fn from(v: u8) -> Self {
        Count { value: v as u32 }
    }
}

impl From<u16> for Count {
    fn from(v: u16) -> Self {
        Count { value: v as u32 }
    }
}
