//! Writing .reg files for `wine regedit /S` and reading values back from Wine's
//! own registry files (system.reg / user.reg in the prefix).
use crate::log::LogSink;
use crate::wine::Wine;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum RegValue {
    Sz(String),
    Dword(u32),
    MultiSz(Vec<String>),
    Binary(Vec<u8>),
    Delete,
}

#[derive(Default, Clone, Debug)]
pub struct RegFile {
    keys: Vec<(String, Vec<(String, RegValue)>)>,
}

impl RegFile {
    pub fn new() -> RegFile {
        RegFile::default()
    }

    /// `key` is a full path such as `HKEY_CURRENT_USER\Software\Wine\Direct2D`.
    pub fn set(&mut self, key: &str, name: &str, v: RegValue) -> &mut Self {
        let idx = match self.keys.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(i) => i,
            None => {
                self.keys.push((key.to_string(), Vec::new()));
                self.keys.len() - 1
            }
        };
        let values = &mut self.keys[idx].1;
        match values.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some(slot) => slot.1 = v,
            None => values.push((name.to_string(), v)),
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn to_regedit_bytes(&self) -> Vec<u8> {
        let mut t = String::from("Windows Registry Editor Version 5.00\r\n");
        for (key, values) in &self.keys {
            t.push_str(&format!("\r\n[{key}]\r\n"));
            for (name, v) in values {
                t.push_str(&format!("\"{}\"={}\r\n", escape(name), encode(v)));
            }
        }
        let mut out = vec![0xFF, 0xFE];
        out.extend(t.encode_utf16().flat_map(u16::to_le_bytes));
        out
    }

    /// Writes the file into `tmp_dir` and imports it with `wine regedit /S`.
    pub fn import(&self, wine: &Wine, log: &LogSink, tmp_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(tmp_dir)?;
        let file = tmp_dir.join("studiowine-import.reg");
        std::fs::write(&file, self.to_regedit_bytes())?;
        let status = wine.run_wait(["regedit", "/S", &crate::wine::dos_path(&file)], log)?;
        let _ = std::fs::remove_file(&file);
        anyhow::ensure!(status.success(), "regedit failed to import registry settings ({status})");
        Ok(())
    }
}

/// Raw right-hand side of `name` under `key` in a Wine registry file
/// (e.g. `"\"arial.ttf\""` or `"dword:00000078"`). `key` uses single backslashes,
/// relative to the file's root; matching is case-insensitive.
pub fn read_value(wine_reg_file: &Path, key: &str, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(wine_reg_file).ok()?;
    let want_key = key.replace('\\', "\\\\").to_lowercase();
    let want_name = format!("\"{}\"=", escape(name)).to_lowercase();
    let mut in_key = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('[') {
            let k = rest.rsplit_once(']').map(|(k, _)| k).unwrap_or(rest);
            in_key = k.to_lowercase() == want_key;
        } else if in_key && line.to_lowercase().starts_with(&want_name) {
            return Some(line[want_name.len()..].to_string());
        }
    }
    None
}

/// Quotes for a .reg string. Control characters are dropped: names come from font
/// files the user supplies, and a CR/LF would start a new key or value line.
fn escape(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().replace('\\', "\\\\").replace('"', "\\\"")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(",")
}

fn encode(v: &RegValue) -> String {
    match v {
        RegValue::Sz(s) => format!("\"{}\"", escape(s)),
        RegValue::Dword(d) => format!("dword:{d:08x}"),
        RegValue::MultiSz(items) => {
            let mut b: Vec<u8> = Vec::new();
            for s in items {
                b.extend(s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
            }
            b.extend([0, 0]);
            format!("hex(7):{}", hex(&b))
        }
        RegValue::Binary(b) => format!("hex:{}", hex(b)),
        RegValue::Delete => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(n: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(n)
    }

    fn decode(b: &[u8]) -> String {
        assert_eq!(&b[..2], &[0xFF, 0xFE], "UTF-16LE BOM");
        let u: Vec<u16> = b[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16(&u).unwrap()
    }

    #[test]
    fn regedit_file_is_utf16_with_header_and_dword() {
        let mut r = RegFile::new();
        r.set(r"HKEY_CURRENT_USER\Software\Wine\Direct2D", "text_linear_blend", RegValue::Dword(1));
        let t = decode(&r.to_regedit_bytes());
        assert!(t.starts_with("Windows Registry Editor Version 5.00\r\n"), "{t}");
        assert!(t.contains("[HKEY_CURRENT_USER\\Software\\Wine\\Direct2D]\r\n"), "{t}");
        assert!(t.contains("\"text_linear_blend\"=dword:00000001\r\n"), "{t}");
    }

    #[test]
    fn values_are_grouped_by_key_and_escaped() {
        let mut r = RegFile::new();
        r.set(r"HKEY_LOCAL_MACHINE\A", "x", RegValue::Sz(r#"C:\a "b""#.into()))
            .set(r"HKEY_LOCAL_MACHINE\B", "y", RegValue::Delete)
            .set(r"HKEY_LOCAL_MACHINE\A", "z", RegValue::Sz("2".into()));
        let t = decode(&r.to_regedit_bytes());
        assert_eq!(t.matches("[HKEY_LOCAL_MACHINE\\A]").count(), 1);
        assert!(t.contains(r#""x"="C:\\a \"b\"""#), "{t}");
        assert!(t.contains("\"y\"=-"), "{t}");
        assert!(t.find("\"z\"").unwrap() < t.find("[HKEY_LOCAL_MACHINE\\B]").unwrap());
    }

    #[test]
    fn control_characters_cannot_inject_lines() {
        let mut r = RegFile::new();
        r.set(r"HKEY_LOCAL_MACHINE\K", "Evil\r\n[HKEY_LOCAL_MACHINE\\X]\r\n\"a\"=\"b", RegValue::Sz("v\nw\u{0}".into()));
        let t = decode(&r.to_regedit_bytes());
        assert_eq!(t.lines().filter(|l| l.starts_with('[')).count(), 1, "{t}");
        assert_eq!(t.matches("\r\n").count(), t.lines().count(), "{t}");
        assert!(!t.contains('\u{0}'));
    }

    #[test]
    fn multi_sz_and_binary_hex() {
        let mut r = RegFile::new();
        r.set(r"HKEY_LOCAL_MACHINE\K", "m", RegValue::MultiSz(vec!["a".into(), "b".into()]))
            .set(r"HKEY_LOCAL_MACHINE\K", "b", RegValue::Binary(vec![0xf4, 0xff, 0x00]));
        let t = decode(&r.to_regedit_bytes());
        assert!(t.contains("\"m\"=hex(7):61,00,00,00,62,00,00,00,00,00\r\n"), "{t}");
        assert!(t.contains("\"b\"=hex:f4,ff,00\r\n"), "{t}");
    }

    #[test]
    fn read_value_from_wine_reg() {
        let f = fixture("system.reg");
        assert_eq!(
            read_value(&f, r"Software\Microsoft\Windows NT\CurrentVersion\Fonts", "Arial (TrueType)").as_deref(),
            Some("\"arial.ttf\"")
        );
        assert_eq!(read_value(&f, r"software\microsoft\windows nt\currentversion\fonts", "arial (truetype)").as_deref(), Some("\"arial.ttf\""));
        assert_eq!(read_value(&fixture("user.reg"), r"Control Panel\Desktop", "LogPixels").as_deref(), Some("dword:00000078"));
        assert_eq!(read_value(&f, r"Software\Nope", "x"), None);
        assert_eq!(read_value(&f, r"Software\Microsoft\Windows NT\CurrentVersion\Fonts", "Missing"), None);
        assert_eq!(read_value(Path::new("/nonexistent.reg"), "a", "b"), None);
    }
}
