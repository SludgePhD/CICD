pub fn next_line<'a>(text: &mut &'a str) -> Option<&'a str> {
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
}
