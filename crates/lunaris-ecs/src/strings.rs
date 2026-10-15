//! String interner. Components are plain data, so text lives here and components hold a `Sym` (u32).
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct StringTable {
    strings: Vec<String>,
    index: BTreeMap<String, u32>,
}

impl Default for StringTable {
    fn default() -> Self {
        let mut t = StringTable { strings: Vec::new(), index: BTreeMap::new() };
        t.strings.push(String::new()); // id 0 is the empty string, which is also the zero value of a Str field
        t.index.insert(String::new(), 0);
        t
    }
}

impl StringTable {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.index.get(s) {
            return i;
        }
        let i = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }

    pub fn get(&self, id: u32) -> &str {
        self.strings.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.len() <= 1
    }
}
