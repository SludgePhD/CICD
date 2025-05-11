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
}
