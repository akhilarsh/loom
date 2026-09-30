mod legacy {
    pub fn clean(text: &str) -> String {
        text.to_string()
    }

    pub fn shout(text: &str) -> String {
        clean(text).to_uppercase()
    }
}

pub fn clean(text: &str) -> String {
    text.trim().to_lowercase()
}

pub fn normalize(name: &str) -> String {
    let cleaned = clean(name);
    legacy::shout(&cleaned)
}

pub fn parse_id(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    trimmed.parse().ok()
}
