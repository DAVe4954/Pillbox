//! The calculator widget: evaluates `2 + 3 * (4 - 1)`, `2^10`, `15 %` style expressions.
//! A small recursive-descent parser; no variables, no functions besides `sqrt`.

/// Evaluate an expression. `Err` explains what is wrong in a few words.
pub fn eval(input: &str) -> Result<f64, String> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err("empty".into());
    }
    let mut p = Parser { tokens, pos: 0 };
    let v = p.expr()?;
    if p.pos != p.tokens.len() {
        return Err("unexpected input".into());
    }
    if !v.is_finite() {
        return Err("not a number".into());
    }
    Ok(v)
}

/// A result as people write it: no trailing zeros, at most 10 decimals, thousands kept plain.
pub fn format(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Num(f64),
    Op(char),
    Open,
    Close,
    Sqrt,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '0'..='9' | '.' | ',' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',')
                {
                    i += 1;
                }
                // A comma is a decimal separator in many locales.
                let text: String = chars[start..i]
                    .iter()
                    .map(|c| if *c == ',' { '.' } else { *c })
                    .collect();
                let n = text
                    .parse::<f64>()
                    .map_err(|_| format!("bad number {text}"))?;
                out.push(Tok::Num(n));
            }
            '+' | '-' | '*' | '/' | '^' | '%' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            '×' | 'x' | 'X' => {
                out.push(Tok::Op('*'));
                i += 1;
            }
            '÷' => {
                out.push(Tok::Op('/'));
                i += 1;
            }
            '(' => {
                out.push(Tok::Open);
                i += 1;
            }
            ')' => {
                out.push(Tok::Close);
                i += 1;
            }
            _ if s[s.char_indices().nth(i).map_or(s.len(), |(b, _)| b)..]
                .to_ascii_lowercase()
                .starts_with("sqrt") =>
            {
                out.push(Tok::Sqrt);
                i += 4;
            }
            '√' => {
                out.push(Tok::Sqrt);
                i += 1;
            }
            _ => return Err(format!("unknown “{c}”")),
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<Tok> {
        self.tokens.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.peek();
        self.pos += 1;
        t
    }

    // expr := term (('+'|'-') term)*
    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek() {
            self.pos += 1;
            let mut rhs = self.term()?;
            // "200 + 10 %" means plus ten percent of 200.
            if self.peek() == Some(Tok::Op('%')) {
                self.pos += 1;
                rhs = v * rhs / 100.0;
            }
            v = if op == '+' { v + rhs } else { v - rhs };
        }
        Ok(v)
    }

    // term := power (('*'|'/') power)*
    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(Tok::Op('*')) => {
                    self.pos += 1;
                    v *= self.power()?;
                }
                Some(Tok::Op('/')) => {
                    self.pos += 1;
                    let d = self.power()?;
                    if d == 0.0 {
                        return Err("division by zero".into());
                    }
                    v /= d;
                }
                // Implicit multiplication: 2(3+4).
                Some(Tok::Open) => v *= self.power()?,
                _ => return Ok(v),
            }
        }
    }

    // power := unary ('^' power)?
    fn power(&mut self) -> Result<f64, String> {
        let base = self.unary()?;
        if self.peek() == Some(Tok::Op('^')) {
            self.pos += 1;
            let exp = self.power()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    // unary := '-' unary | atom ['%' when not followed by an operand]
    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        let v = match self.next() {
            Some(Tok::Num(n)) => n,
            Some(Tok::Open) => {
                let v = self.expr()?;
                if self.next() != Some(Tok::Close) {
                    return Err("missing )".into());
                }
                v
            }
            Some(Tok::Sqrt) => {
                let v = self.unary()?;
                if v < 0.0 {
                    return Err("square root of a negative".into());
                }
                v.sqrt()
            }
            _ => return Err("incomplete".into()),
        };
        // A trailing % on its own means "per cent": 15% = 0.15. In "a + b %" the caller
        // handles it, so only take it here when nothing additive came before.
        if self.peek() == Some(Tok::Op('%')) && !self.after_additive() {
            self.pos += 1;
            return Ok(v / 100.0);
        }
        Ok(v)
    }

    /// Whether the number just read is the right side of a + or -.
    fn after_additive(&self) -> bool {
        self.pos >= 2
            && matches!(self.tokens.get(self.pos - 2), Some(Tok::Op('+' | '-')))
            && self.pos >= 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> f64 {
        match eval(s) {
            Ok(v) => v,
            Err(e) => panic!("{s}: {e}"),
        }
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ok("1 + 2 * 3"), 7.0);
        assert_eq!(ok("(1 + 2) * 3"), 9.0);
        assert_eq!(ok("2^10"), 1024.0);
        assert_eq!(ok("2^3^2"), 512.0, "power is right-associative");
        assert_eq!(ok("-3 + 5"), 2.0);
        assert_eq!(ok("10 / 4"), 2.5);
        assert_eq!(ok("2(3+4)"), 14.0);
        assert_eq!(ok("3 × 4 ÷ 2"), 6.0);
        assert_eq!(ok("1,5 * 2"), 3.0);
        assert_eq!(ok("sqrt 16 + 1"), 5.0);
    }

    #[test]
    fn percentages() {
        assert_eq!(ok("15%"), 0.15);
        assert_eq!(ok("200 + 10%"), 220.0);
        assert_eq!(ok("200 - 25%"), 150.0);
        assert_eq!(ok("50% * 8"), 4.0);
    }

    #[test]
    fn errors() {
        for s in [
            "", "1 +", "(1 + 2", "1 / 0", "2 ** 3", "abc", "sqrt -4", "1 2",
        ] {
            assert!(eval(s).is_err(), "{s:?} should fail");
        }
    }

    #[test]
    fn formatting() {
        assert_eq!(format(4.0), "4");
        assert_eq!(format(2.5), "2.5");
        assert_eq!(format(1.0 / 3.0), "0.3333333333");
        assert_eq!(format(-12.0), "-12");
    }
}
