//! Search/filter support for every tab's search box (Ctrl+K) and the
//! Find Open Files view (Ctrl+F). Supports plain substring and `*`/`?`
//! wildcards, case-insensitive by default.

#[derive(Clone, Debug)]
pub struct Matcher {
    raw: String,
    tokens: Vec<Token>,
    plain: bool,
    needle_lower: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Lit(char),
    Any,    // ?
    Star,   // *
}

impl Matcher {
    pub fn new(pattern: &str) -> Self {
        let p = pattern.trim();
        let plain = !p.contains('*') && !p.contains('?');
        let tokens = p
            .chars()
            .flat_map(|c| c.to_lowercase())
            .map(|c| match c {
                '*' => Token::Star,
                '?' => Token::Any,
                c => Token::Lit(c),
            })
            .collect();
        Self {
            raw: p.to_string(),
            tokens,
            plain,
            needle_lower: p.to_lowercase(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn matches(&self, haystack: &str) -> bool {
        if self.raw.is_empty() {
            return true;
        }
        let hay: Vec<char> = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
        if self.plain {
            return haystack.to_lowercase().contains(&self.needle_lower);
        }
        wildcard_match(&self.tokens, &hay)
    }
}

fn wildcard_match(pat: &[Token], text: &[char]) -> bool {
    // Classic two-pointer glob with backtracking on '*'.
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star_p, mut star_t) = (usize::MAX, 0usize);
    while t < text.len() {
        if p < pat.len() && (pat[p] == Token::Any || pat[p] == Token::Lit(text[t])) {
            p += 1;
            t += 1;
        } else if p < pat.len() && pat[p] == Token::Star {
            star_p = p;
            star_t = t;
            p += 1;
        } else if star_p != usize::MAX {
            p = star_p + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == Token::Star {
        p += 1;
    }
    p == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_substring() {
        let m = Matcher::new("safari");
        assert!(m.matches("/Applications/Safari.app"));
        assert!(m.matches("com.apple.Safari"));
        assert!(!m.matches("firefox"));
    }

    #[test]
    fn wildcards() {
        assert!(Matcher::new("*.dylib").matches("/usr/lib/libSystem.B.dylib"));
        assert!(Matcher::new("lib*").matches("libobjc"));
        assert!(Matcher::new("disk?").matches("disk0"));
        assert!(!Matcher::new("disk?").matches("disk10"));
        assert!(Matcher::new("*safari*helper*").matches("Safari Web Helper"));
        assert!(Matcher::new("*").matches("anything"));
    }

    #[test]
    fn empty_matches_all() {
        assert!(Matcher::new("").matches("whatever"));
        assert!(Matcher::new("   ").matches("whatever"));
    }
}
