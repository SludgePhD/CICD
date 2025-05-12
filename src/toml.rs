use crate::Result;

pub enum Value<'a> {
    Str(&'a str),
    Bool(bool),
}

impl<'a> Value<'a> {
    pub fn as_str(&self) -> Option<&'a str> {
        if let Self::Str(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

pub struct Toml<'a>(pub &'a str);

impl<'a> Toml<'a> {
    pub fn get_field(&self, name: &str) -> Result<Value<'a>> {
        for line in self.0.lines() {
            let words = line.split_ascii_whitespace().collect::<Vec<_>>();
            match words.as_slice() {
                [n, "=", v, ..] if n.trim() == name => {
                    let v = v.trim();
                    if v.starts_with('"') {
                        assert!(
                            v.ends_with('"'),
                            "unclosed string, or trailing comment in '{line}'"
                        );
                        return Ok(Value::Str(&v[1..v.len() - 1]));
                    } else if v.split(|v: char| !v.is_alphanumeric()).next().unwrap() == "true" {
                        return Ok(Value::Bool(true));
                    } else if v.split(|v: char| !v.is_alphanumeric()).next().unwrap() == "false" {
                        return Ok(Value::Bool(false));
                    }
                }
                _ => (),
            }
        }
        Err(format!(
            "can't find `{}` in\n----\n{}\n----\n",
            name, self.0
        ))?
    }

    pub fn sections(&self) -> Vec<(&str, Toml<'_>)> {
        // `Lines` has no stable `remainder` or `as_str` method, so we have to do this manually...
        let mut remainder = self.0;

        // Find the first line that starts a section.
        let mut section_name = loop {
            let Some(line) = next_line(&mut remainder) else {
                return Vec::new();
            };
            if let Some(name) = start_of_section_or_array(line) {
                break name;
            }
        };

        let mut section_contents = &self.0[self.0.len() - remainder.len()..];
        let mut out = Vec::new();
        // Find the beginning of the next section, or the end of the file.
        loop {
            let contents = section_contents[..section_contents.len() - remainder.len()].trim();
            let Some(line) = next_line(&mut remainder) else {
                out.push((section_name, Toml(contents)));
                break;
            };
            if let Some(name) = start_of_section_or_array(line) {
                out.push((section_name, Toml(contents)));
                section_name = name;
                section_contents = &self.0[self.0.len() - remainder.len()..];
            }
        }

        out
    }
}

fn start_of_section_or_array(mut line: &str) -> Option<&str> {
    line = line.trim();

    if !line.starts_with('[') {
        return None;
    }

    if line.starts_with("[[") {
        line = &line[2..];
    } else {
        line = &line[1..];
    }

    let (name, _) = line.split_once(']')?;
    Some(name.trim())
}

fn next_line<'a>(text: &mut &'a str) -> Option<&'a str> {
    match text.split_once('\n') {
        Some((line, rest)) => {
            *text = rest;
            Some(line)
        }
        None => {
            if text.is_empty() {
                return None;
            } else {
                let line = *text;
                *text = "";
                Some(line)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;

    #[test]
    fn test_next_line() {
        #[track_caller]
        fn check(mut input: &str, line: Option<&str>, rest: &str) {
            assert_eq!(next_line(&mut input), line);
            assert_eq!(input, rest);
        }

        check("a\nb", Some("a"), "b");
        check("abc", Some("abc"), "");
        check("\n", Some(""), "");
        check("\nxyz", Some(""), "xyz");
        check("", None, "");
    }

    #[test]
    fn test_start_of_section() {
        assert_eq!(start_of_section_or_array("[section]"), Some("section"));
        assert_eq!(start_of_section_or_array("[[array]]"), Some("array"));
        assert_eq!(start_of_section_or_array(" [section]"), Some("section"));
        assert_eq!(start_of_section_or_array(" [[array]]"), Some("array"));
        assert_eq!(
            start_of_section_or_array("[section] # comment"),
            Some("section")
        );
        assert_eq!(
            start_of_section_or_array("[[array]] # comment"),
            Some("array")
        );
        assert_eq!(
            start_of_section_or_array(r#"[bla."*".asd]"#),
            Some(r#"bla."*".asd"#)
        );
    }

    #[test]
    fn test_sections() {
        let toml = Toml(
            r#"
            [[array]]
            a = 0
            [[array]]
            b = 1
            [empty]
            [package]
            version = "1.0.0"  # comment
            [dependencies]
            bla = { version = "*", path = "bla" }
            [target.x86_64-pc-windows-gnu."*".dependencies]
            eof = true
            "#,
        );

        let sections = toml
            .sections()
            .into_iter()
            .map(|(name, toml)| (name, toml.0))
            .collect::<Vec<_>>();
        expect![[r#"
            [
                (
                    "array",
                    "a = 0",
                ),
                (
                    "array",
                    "b = 1",
                ),
                (
                    "empty",
                    "",
                ),
                (
                    "package",
                    "version = \"1.0.0\"  # comment",
                ),
                (
                    "dependencies",
                    "bla = { version = \"*\", path = \"bla\" }",
                ),
                (
                    "target.x86_64-pc-windows-gnu.\"*\".dependencies",
                    "eof = true",
                ),
            ]
        "#]]
        .assert_debug_eq(&sections);
    }
}
