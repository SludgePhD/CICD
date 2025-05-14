use std::fmt;

use crate::utils::next_line;

pub struct Markdown<'a>(pub &'a str);

impl<'a> fmt::Debug for Markdown<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> Markdown<'a> {
    pub fn sections(&self, level: u8) -> Vec<(&'a str, Markdown<'a>)> {
        // `Lines` has no stable `remainder` or `as_str` method, so we have to do this manually...
        let mut remainder = self.0;

        // Find the first line that starts a section.
        let mut section_name = loop {
            let Some(line) = next_line(&mut remainder) else {
                return Vec::new();
            };
            if let Some(name) = start_of_section(level, line) {
                break name;
            }
        };

        let mut section_contents = &self.0[self.0.len() - remainder.len()..];
        let mut out = Vec::new();
        // Find the beginning of the next section, or the end of the file.
        loop {
            let contents = section_contents[..section_contents.len() - remainder.len()].trim();
            let Some(line) = next_line(&mut remainder) else {
                out.push((section_name, Markdown(contents)));
                break;
            };
            if let Some(name) = start_of_section(level, line) {
                out.push((section_name, Markdown(contents)));
                section_name = name;
                section_contents = &self.0[self.0.len() - remainder.len()..];
            }
        }

        out
    }
}

fn start_of_section(level: u8, mut line: &str) -> Option<&str> {
    line = line.trim();

    if !line.starts_with('#') {
        return None;
    }

    let trimmed = line.trim_start_matches('#');
    let lvl = line.len() - trimmed.len();
    if lvl == level.into() {
        Some(trimmed.trim())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;

    #[test]
    fn test_sections() {
        let markdown = Markdown(
            r#"
                # 1
                bla
                ## 2.1
                ## 2.2
                aa
                ## 2.3

                - a
                - b
            "#,
        );
        let sections = markdown.sections(2);

        expect![[r#"
            [
                (
                    "2.1",
                    "",
                ),
                (
                    "2.2",
                    "aa",
                ),
                (
                    "2.3",
                    "- a\n                - b",
                ),
            ]
        "#]]
        .assert_debug_eq(&sections);

        let sections = markdown.sections(3);
        expect![[r#"
            []
        "#]]
        .assert_debug_eq(&sections);
    }
}
