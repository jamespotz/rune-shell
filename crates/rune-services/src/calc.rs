//! Calculator for the launcher: `+ - * / % ^`, parentheses, `pi`, `e`, and
//! `sqrt sin cos tan ln log abs`.

/// Evaluates `input` if it looks like a calculation (a bare number or word
/// does not), returning the formatted result.
pub fn eval(input: &str) -> Option<String> {
    let input = input.trim();
    let mathy =
        input.chars().any(|c| "+-*/%^(".contains(c)) && input.chars().any(|c| c.is_ascii_digit());
    if !mathy {
        return None;
    }
    let mut p = Parser {
        s: input.as_bytes(),
        i: 0,
    };
    let value = p.expr()?;
    p.skip_ws();
    (p.i == p.s.len() && value.is_finite()).then(|| format(value))
}

fn format(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{v:.0}");
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.skip_ws();
        let hit = self.s.get(self.i) == Some(&c);
        if hit {
            self.i += 1;
        }
        hit
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            if self.eat(b'*') {
                v *= self.power()?;
            } else if self.eat(b'/') {
                v /= self.power()?;
            } else if self.eat(b'%') {
                v %= self.power()?;
            } else {
                return Some(v);
            }
        }
    }

    /// Right-associative: `2^3^2` is `2^9`.
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.eat(b'^') {
            Some(base.powf(self.power()?))
        } else {
            Some(base)
        }
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        self.eat(b'+');
        self.primary()
    }

    fn primary(&mut self) -> Option<f64> {
        self.skip_ws();
        if self.eat(b'(') {
            let v = self.expr()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        while self
            .s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.i += 1;
        }
        if self.i > start {
            return std::str::from_utf8(&self.s[start..self.i])
                .ok()?
                .parse()
                .ok();
        }
        while self.s.get(self.i).is_some_and(u8::is_ascii_alphabetic) {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        let f: fn(f64) -> f64 = match name {
            "pi" => return Some(std::f64::consts::PI),
            "e" => return Some(std::f64::consts::E),
            "sqrt" => f64::sqrt,
            "sin" => f64::sin,
            "cos" => f64::cos,
            "tan" => f64::tan,
            "ln" => f64::ln,
            "log" => f64::log10,
            "abs" => f64::abs,
            _ => return None,
        };
        if !self.eat(b'(') {
            return None;
        }
        let v = self.expr()?;
        self.eat(b')').then(|| f(v))
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn evaluates() {
        assert_eq!(eval("1 + 2 * 3").as_deref(), Some("7"));
        assert_eq!(eval("(1 + 2) * 3").as_deref(), Some("9"));
        assert_eq!(eval("7 / 2").as_deref(), Some("3.5"));
        assert_eq!(eval("2^3^2").as_deref(), Some("512"));
        assert_eq!(eval("-3 + 10 % 4").as_deref(), Some("-1"));
        assert_eq!(eval("sqrt(16) + 2*pi").as_deref(), Some("10.2831853072"));
        assert_eq!(eval("0.1 + 0.2").as_deref(), Some("0.3"));
    }

    #[test]
    fn ignores_non_math() {
        assert_eq!(eval("firefox"), None);
        assert_eq!(eval("42"), None);
        assert_eq!(eval("1 +"), None);
        assert_eq!(eval("1 / 0"), None);
        assert_eq!(eval("foo(2)"), None);
        assert_eq!(eval("(1 + 2"), None);
    }
}
