//! `.gdt` files: the mod tools' asset definitions as text. Each entry is
//! `"name" ( "type.gdf" ) { "key" "value" ... }`, optionally with a parent instead of a type
//! (`"name" [ "parent" ]`), which this reader keeps as the type.

use std::collections::HashMap;
use std::path::Path;

use crate::Result;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    /// The `.gdf` type (or the parent entry's name).
    pub kind: String,
    pub fields: HashMap<String, String>,
}

impl Entry {
    pub fn str(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }

    pub fn f32(&self, key: &str) -> Option<f32> {
        self.str(key)?.trim().parse().ok()
    }
}

pub type Gdt = HashMap<String, Entry>;

pub fn load(path: impl AsRef<Path>) -> Result<Gdt> {
    Ok(parse(&std::fs::read_to_string(path)?))
}

/// Quoted strings, in order; brackets and braces are structure.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < b.len() && b[j] != b'"' {
                    j += 1;
                }
                out.push(Token::Str(&text[start..j.min(b.len())]));
                i = j + 1;
            }
            b'{' => {
                out.push(Token::Open);
                i += 1;
            }
            b'}' => {
                out.push(Token::Close);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

enum Token<'a> {
    Str(&'a str),
    Open,
    Close,
}

pub fn parse(text: &str) -> Gdt {
    let mut gdt = Gdt::new();
    let toks = tokens(text);
    let mut i = 0;
    // Skip the file's own outer braces; entries are "name" "type" { pairs }.
    while i < toks.len() {
        match (&toks[i], toks.get(i + 1), toks.get(i + 2)) {
            (Token::Str(name), Some(Token::Str(kind)), Some(Token::Open)) => {
                let mut entry = Entry { kind: kind.to_string(), fields: HashMap::new() };
                i += 3;
                while i + 1 < toks.len() {
                    match (&toks[i], &toks[i + 1]) {
                        (Token::Str(k), Token::Str(v)) => {
                            entry.fields.insert(k.to_string(), v.to_string());
                            i += 2;
                        }
                        _ => break,
                    }
                }
                if matches!(toks.get(i), Some(Token::Close)) {
                    i += 1;
                }
                gdt.insert(name.to_string(), entry);
            }
            _ => i += 1,
        }
    }
    gdt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_and_fields() {
        let text = r#"{
	"au_x" ( "attachmentunique.gdf" )
	{
		"a" "1"
	}
	"smg_standard_mp" ( "bulletweapon.gdf" )
	{
		"fireTime" "0.0833"
		"clipSize" "30"
		"displayName" "WEAPON_SMG_STANDARD"
	}
}"#;
        let g = parse(text);
        let w = &g["smg_standard_mp"];
        assert_eq!(w.kind, "bulletweapon.gdf");
        assert_eq!(w.f32("fireTime"), Some(0.0833));
        assert_eq!(w.f32("clipSize"), Some(30.0));
        assert_eq!(w.str("displayName"), Some("WEAPON_SMG_STANDARD"));
        assert_eq!(g["au_x"].str("a"), Some("1"));
    }
}
