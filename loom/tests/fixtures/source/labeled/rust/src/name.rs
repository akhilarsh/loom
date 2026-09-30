pub struct Name(pub String);

impl From<Name> for String {
    fn from(name: Name) -> String {
        name.0
    }
}

pub fn greeting() -> String {
    String::from("hi")
}
