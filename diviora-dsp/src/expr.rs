//! Expressions for Equalizer APO's `If:`/`ElseIf:`, `Eval:` and `backtick` substitutions.
//!
//! Evaluated when the configuration is compiled for a concrete output, with the same
//! built-in variables Equalizer APO provides: `sampleRate`, `channelCount`,
//! `inputChannelCount`, `outputChannelCount`, `deviceName`, `connectionName`,
//! `deviceGuid`, `deviceString`, `stage`, plus variables set with `Eval:`.
//!
//! Syntax (C-like): numbers, "strings" / 'strings', variables, `( )`, unary `- ! not`,
//! `^` (power), `* / %`, `+ -` (`+` joins strings), `< <= > >=`, `== !=` (strings compare
//! case-insensitively), `&& and`, `|| or`, `cond ? a : b`, and functions:
//! abs sqrt pow exp log log10 ln sin cos tan asin acos atan atan2 floor ceil round min max
//! regexSearch regexMatch regexReplace contains startsWith endsWith length lower upper
//! toLowerCase toUpperCase toNumber toString.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Str(String),
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty() && s != "0" && !s.eq_ignore_ascii_case("false"),
        }
    }
    pub fn num(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::Str(s) => s.trim().parse().unwrap_or(f64::NAN),
        }
    }
    pub fn text(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Num(n) => format_num(*n),
        }
    }
}

/// Numbers as Equalizer APO writes them into commands: no trailing ".0" for integers.
pub fn format_num(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{:.10}", n);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub type Vars = HashMap<String, Value>;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).map_or(false, |d| d.is_ascii_digit())) {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                let save = i;
                i += 1;
                if i < c.len() && (c[i] == '+' || c[i] == '-') { i += 1; }
                if i < c.len() && c[i].is_ascii_digit() {
                    while i < c.len() && c[i].is_ascii_digit() { i += 1; }
                } else {
                    i = save;
                }
            }
            let t: String = c[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("bad number '{}'", t))?));
            continue;
        }
        if ch == '"' || ch == '\'' {
            let q = ch;
            i += 1;
            let mut t = String::new();
            while i < c.len() && c[i] != q {
                if c[i] == '\\' && i + 1 < c.len() {
                    i += 1;
                    t.push(match c[i] { 'n' => '\n', 't' => '\t', o => o });
                } else {
                    t.push(c[i]);
                }
                i += 1;
            }
            if i >= c.len() { return Err("unterminated string".into()); }
            i += 1;
            out.push(Tok::Str(t));
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') { i += 1; }
            out.push(Tok::Ident(c[st..i].iter().collect()));
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        let op2 = ["==", "!=", "<=", ">=", "&&", "||"].iter().find(|o| **o == two);
        if let Some(o) = op2 {
            out.push(Tok::Op(o));
            i += 2;
            continue;
        }
        let t = match ch {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            '?' => Tok::Question,
            ':' => Tok::Colon,
            '+' => Tok::Op("+"),
            '-' => Tok::Op("-"),
            '*' => Tok::Op("*"),
            '/' => Tok::Op("/"),
            '%' => Tok::Op("%"),
            '^' => Tok::Op("^"),
            '<' => Tok::Op("<"),
            '>' => Tok::Op(">"),
            '!' => Tok::Op("!"),
            '=' => Tok::Op("=="), // a single '=' in a condition means comparison
            _ => return Err(format!("unexpected character '{}'", ch)),
        };
        out.push(t);
        i += 1;
    }
    Ok(out)
}

struct P<'a> {
    t: Vec<Tok>,
    i: usize,
    vars: &'a Vars,
}

impl<'a> P<'a> {
    fn peek(&self) -> Option<&Tok> { self.t.get(self.i) }
    fn next(&mut self) -> Option<Tok> { let t = self.t.get(self.i).cloned(); self.i += 1; t }
    fn is_op(&self, ops: &[&str]) -> Option<&'static str> {
        match self.peek() {
            Some(Tok::Op(o)) if ops.contains(o) => Some(*o),
            Some(Tok::Ident(w)) if ops.contains(&"&&") && w.eq_ignore_ascii_case("and") => Some("&&"),
            Some(Tok::Ident(w)) if ops.contains(&"||") && w.eq_ignore_ascii_case("or") => Some("||"),
            _ => None,
        }
    }

    fn ternary(&mut self) -> Result<Value, String> {
        let c = self.or()?;
        if matches!(self.peek(), Some(Tok::Question)) {
            self.i += 1;
            let a = self.ternary()?;
            if !matches!(self.next(), Some(Tok::Colon)) { return Err("expected ':' in '? :'".into()); }
            let b = self.ternary()?;
            return Ok(if c.truthy() { a } else { b });
        }
        Ok(c)
    }
    fn or(&mut self) -> Result<Value, String> {
        let mut l = self.and()?;
        while self.is_op(&["||"]).is_some() {
            self.i += 1;
            let r = self.and()?;
            l = Value::Num((l.truthy() || r.truthy()) as i32 as f64);
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Value, String> {
        let mut l = self.eq()?;
        while self.is_op(&["&&"]).is_some() {
            self.i += 1;
            let r = self.eq()?;
            l = Value::Num((l.truthy() && r.truthy()) as i32 as f64);
        }
        Ok(l)
    }
    fn eq(&mut self) -> Result<Value, String> {
        let mut l = self.cmp()?;
        while let Some(op) = self.is_op(&["==", "!="]) {
            self.i += 1;
            let r = self.cmp()?;
            let same = match (&l, &r) {
                (Value::Num(a), Value::Num(b)) => (a - b).abs() < 1e-9,
                _ => l.text().eq_ignore_ascii_case(&r.text()),
            };
            l = Value::Num(((op == "==") == same) as i32 as f64);
        }
        Ok(l)
    }
    fn cmp(&mut self) -> Result<Value, String> {
        let mut l = self.add()?;
        while let Some(op) = self.is_op(&["<", "<=", ">", ">="]) {
            self.i += 1;
            let r = self.add()?;
            let o = match (&l, &r) {
                (Value::Str(a), Value::Str(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
                _ => l.num().partial_cmp(&r.num()).unwrap_or(std::cmp::Ordering::Equal),
            };
            use std::cmp::Ordering::*;
            let t = match op { "<" => o == Less, "<=" => o != Greater, ">" => o == Greater, _ => o != Less };
            l = Value::Num(t as i32 as f64);
        }
        Ok(l)
    }
    fn add(&mut self) -> Result<Value, String> {
        let mut l = self.mul()?;
        while let Some(op) = self.is_op(&["+", "-"]) {
            self.i += 1;
            let r = self.mul()?;
            l = match (op, &l, &r) {
                ("+", Value::Str(_), _) | ("+", _, Value::Str(_)) => Value::Str(l.text() + &r.text()),
                ("+", _, _) => Value::Num(l.num() + r.num()),
                _ => Value::Num(l.num() - r.num()),
            };
        }
        Ok(l)
    }
    fn mul(&mut self) -> Result<Value, String> {
        let mut l = self.unary()?;
        while let Some(op) = self.is_op(&["*", "/", "%"]) {
            self.i += 1;
            let r = self.unary()?;
            let (a, b) = (l.num(), r.num());
            l = Value::Num(match op { "*" => a * b, "/" => a / b, _ => a % b });
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Value, String> {
        if let Some(op) = self.is_op(&["-", "!", "+"]) {
            self.i += 1;
            let v = self.unary()?;
            return Ok(match op { "-" => Value::Num(-v.num()), "!" => Value::Num((!v.truthy()) as i32 as f64), _ => v });
        }
        if let Some(Tok::Ident(w)) = self.peek() {
            if w.eq_ignore_ascii_case("not") {
                self.i += 1;
                let v = self.unary()?;
                return Ok(Value::Num((!v.truthy()) as i32 as f64));
            }
        }
        self.pow()
    }
    fn pow(&mut self) -> Result<Value, String> {
        let b = self.primary()?;
        if self.is_op(&["^"]).is_some() {
            self.i += 1;
            let e = self.unary()?;
            return Ok(Value::Num(b.num().powf(e.num())));
        }
        Ok(b)
    }
    fn primary(&mut self) -> Result<Value, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Value::Num(n)),
            Some(Tok::Str(s)) => Ok(Value::Str(s)),
            Some(Tok::LParen) => {
                let v = self.ternary()?;
                if !matches!(self.next(), Some(Tok::RParen)) { return Err("missing ')'".into()); }
                Ok(v)
            }
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.ternary()?);
                            match self.next() {
                                Some(Tok::Comma) => continue,
                                Some(Tok::RParen) => break,
                                _ => return Err(format!("expected ',' or ')' in {}(...)", name)),
                            }
                        }
                    } else {
                        self.i += 1;
                    }
                    return call(&name, &args);
                }
                match name.to_lowercase().as_str() {
                    "true" => return Ok(Value::Num(1.0)),
                    "false" => return Ok(Value::Num(0.0)),
                    "pi" => return Ok(Value::Num(std::f64::consts::PI)),
                    _ => {}
                }
                self.vars
                    .get(&name)
                    .or_else(|| self.vars.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name)).map(|(_, v)| v))
                    .cloned()
                    .ok_or_else(|| format!("unknown variable '{}'", name))
            }
            other => Err(format!("unexpected {:?}", other)),
        }
    }
}

fn regex(pat: &str) -> Result<regex_lite::Regex, String> {
    regex_lite::Regex::new(&format!("(?i){}", pat)).map_err(|e| format!("bad regular expression: {}", e))
}

fn call(name: &str, a: &[Value]) -> Result<Value, String> {
    let n = |i: usize| a.get(i).map(|v| v.num()).unwrap_or(f64::NAN);
    let s = |i: usize| a.get(i).map(|v| v.text()).unwrap_or_default();
    let want = |k: usize| if a.len() < k { Err(format!("{}() needs {} argument(s)", name, k)) } else { Ok(()) };
    let lname = name.to_lowercase();
    let v = match lname.as_str() {
        "abs" => { want(1)?; Value::Num(n(0).abs()) }
        "sqrt" => { want(1)?; Value::Num(n(0).sqrt()) }
        "pow" => { want(2)?; Value::Num(n(0).powf(n(1))) }
        "exp" => { want(1)?; Value::Num(n(0).exp()) }
        "log" | "ln" => { want(1)?; Value::Num(n(0).ln()) }
        "log10" => { want(1)?; Value::Num(n(0).log10()) }
        "log2" => { want(1)?; Value::Num(n(0).log2()) }
        "sin" => { want(1)?; Value::Num(n(0).sin()) }
        "cos" => { want(1)?; Value::Num(n(0).cos()) }
        "tan" => { want(1)?; Value::Num(n(0).tan()) }
        "asin" => { want(1)?; Value::Num(n(0).asin()) }
        "acos" => { want(1)?; Value::Num(n(0).acos()) }
        "atan" => { want(1)?; Value::Num(n(0).atan()) }
        "atan2" => { want(2)?; Value::Num(n(0).atan2(n(1))) }
        "floor" => { want(1)?; Value::Num(n(0).floor()) }
        "ceil" => { want(1)?; Value::Num(n(0).ceil()) }
        "round" => { want(1)?; Value::Num(n(0).round()) }
        "min" => { want(1)?; Value::Num(a.iter().map(|v| v.num()).fold(f64::INFINITY, f64::min)) }
        "max" => { want(1)?; Value::Num(a.iter().map(|v| v.num()).fold(f64::NEG_INFINITY, f64::max)) }
        "regexsearch" => { want(2)?; Value::Num(regex(&s(1))?.is_match(&s(0)) as i32 as f64) }
        "regexmatch" => { want(2)?; Value::Num(regex(&format!("^(?:{})$", s(1)))?.is_match(&s(0)) as i32 as f64) }
        "regexreplace" => { want(3)?; Value::Str(regex(&s(1))?.replace_all(&s(0), s(2).as_str()).into_owned()) }
        "contains" => { want(2)?; Value::Num(s(0).to_lowercase().contains(&s(1).to_lowercase()) as i32 as f64) }
        "startswith" => { want(2)?; Value::Num(s(0).to_lowercase().starts_with(&s(1).to_lowercase()) as i32 as f64) }
        "endswith" => { want(2)?; Value::Num(s(0).to_lowercase().ends_with(&s(1).to_lowercase()) as i32 as f64) }
        "length" | "len" => { want(1)?; Value::Num(s(0).chars().count() as f64) }
        "lower" | "tolowercase" => { want(1)?; Value::Str(s(0).to_lowercase()) }
        "upper" | "touppercase" => { want(1)?; Value::Str(s(0).to_uppercase()) }
        "tonumber" | "number" => { want(1)?; Value::Num(n(0)) }
        "tostring" | "string" => { want(1)?; Value::Str(s(0)) }
        _ => return Err(format!("unknown function '{}'", name)),
    };
    Ok(v)
}

/// Evaluate one expression.
pub fn eval(src: &str, vars: &Vars) -> Result<Value, String> {
    let t = tokenize(src)?;
    if t.is_empty() {
        return Err("empty expression".into());
    }
    let mut p = P { t, i: 0, vars };
    let v = p.ternary()?;
    if p.i < p.t.len() {
        return Err(format!("unexpected text after the expression: {:?}", p.t[p.i]));
    }
    Ok(v)
}

/// `Eval:` line: one or more `name = expression` assignments separated by `;`.
pub fn eval_assignments(args: &str, vars: &mut Vars) -> Result<(), String> {
    for part in args.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, ex) = part.split_once('=').ok_or_else(|| format!("'{}' is not an assignment (name = value)", part))?;
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(format!("bad variable name '{}'", name));
        }
        let v = eval(ex, vars)?;
        vars.insert(name.to_string(), v);
    }
    Ok(())
}

/// Replace every `expression` (in backticks) in a line with its value.
pub fn substitute_backticks(line: &str, vars: &Vars) -> Result<String, String> {
    if !line.contains('`') {
        return Ok(line.to_string());
    }
    let mut out = String::new();
    let mut rest = line;
    while let Some(a) = rest.find('`') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        let b = after.find('`').ok_or("unmatched backtick")?;
        out.push_str(&eval(&after[..b], vars)?.text());
        rest = &after[b + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        let mut v = Vars::new();
        v.insert("sampleRate".into(), Value::Num(48000.0));
        v.insert("deviceName".into(), Value::Str("Speakers (Realtek(R) Audio)".into()));
        v.insert("channelCount".into(), Value::Num(2.0));
        v
    }

    #[test]
    fn conditions() {
        let v = vars();
        assert!(eval("sampleRate == 48000", &v).unwrap().truthy());
        assert!(eval("sampleRate >= 44100 && channelCount == 2", &v).unwrap().truthy());
        assert!(!eval("sampleRate == 44100 or channelCount > 2", &v).unwrap().truthy());
        assert!(eval("regexSearch(deviceName, \"realtek\")", &v).unwrap().truthy());
        assert!(eval("deviceName == 'speakers (realtek(r) audio)'", &v).unwrap().truthy());
        assert!(eval("!(channelCount != 2)", &v).unwrap().truthy());
        assert_eq!(eval("2 ^ 3 * 2 + 1", &v).unwrap(), Value::Num(17.0));
        assert_eq!(eval("sampleRate > 44100 ? -3 : -6", &v).unwrap(), Value::Num(-3.0));
        assert!(eval("nope == 1", &v).is_err());
    }

    #[test]
    fn eval_and_backticks() {
        let mut v = vars();
        eval_assignments("gain = -3.5; f = sampleRate / 480", &mut v).unwrap();
        assert_eq!(substitute_backticks("Filter: ON PK Fc `f` Hz Gain `gain*2` dB Q 1", &v).unwrap(),
                   "Filter: ON PK Fc 100 Hz Gain -7 dB Q 1");
        assert_eq!(format_num(0.25), "0.25");
    }
}
