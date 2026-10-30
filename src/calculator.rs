use crate::Result;

/// Arithmetic with postfix percent (50% = 0.5), unary signs and parentheses.
/// No evaluation of code, functions or variables other than `ans`.
pub fn evaluate(input: &str, ans: f64) -> Result<f64> {
    if input.len() > 4096 {
        return Err("Expression is too long (maximum 4096 bytes)".into());
    }
    let expanded = input.replace("ans", &format!("({ans})"));
    let mut parser = Parser {
        text: expanded.as_bytes(),
        pos: 0,
        depth: 0,
    };
    let value = parser.expr(0)?;
    parser.space();
    if parser.pos != parser.text.len() {
        return Err("Unexpected input".into());
    }
    finite(value)
}

fn finite(value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Result is not finite (check division by zero or overflow)".into())
    }
}

struct Parser<'a> {
    text: &'a [u8],
    pos: usize,
    depth: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.text.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }
    fn expr(&mut self, minimum: u8) -> Result<f64> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("Expression is nested too deeply".into());
        }
        self.space();
        let mut value = match self.text.get(self.pos).copied() {
            Some(b'+' | b'-') => {
                let negative = self.text[self.pos] == b'-';
                self.pos += 1;
                let n = self.expr(3)?;
                if negative {
                    -n
                } else {
                    n
                }
            }
            Some(b'(') => {
                self.pos += 1;
                let n = self.expr(0)?;
                self.space();
                if self.text.get(self.pos) != Some(&b')') {
                    return Err("Missing closing parenthesis".into());
                }
                self.pos += 1;
                n
            }
            _ => {
                let start = self.pos;
                while self
                    .text
                    .get(self.pos)
                    .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
                {
                    self.pos += 1;
                }
                let number = std::str::from_utf8(&self.text[start..self.pos])
                    .unwrap_or("")
                    .parse::<f64>()
                    .map_err(|_| "Expected a number".to_string())?;
                finite(number)?
            }
        };
        loop {
            self.space();
            if self.text.get(self.pos) == Some(&b'%') {
                self.pos += 1;
                value = finite(value / 100.0)?;
                continue;
            }
            let Some(op) = self.text.get(self.pos).copied() else {
                break;
            };
            let precedence = match op {
                b'+' | b'-' => 1,
                b'*' | b'/' => 2,
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.pos += 1;
            let right = self.expr(precedence + 1)?;
            value = finite(match op {
                b'+' => value + right,
                b'-' => value - right,
                b'*' => value * right,
                _ => value / right,
            })?;
        }
        self.depth -= 1;
        Ok(value)
    }
}
