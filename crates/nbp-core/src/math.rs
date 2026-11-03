//! The math channel: an expression over the samples, evaluated on every one.
//!
//! ```text
//! variables   I (A), V (V), P (W), t (s)
//! operators   + - * / ^ (power), unary -, parentheses
//! numbers     1.5, 2e-3, with an SI prefix: 10m, 1u, 1µ, 2k (a unit after it is ignored: 1ms)
//! functions   abs sqrt exp ln log10   min(a, b) max(a, b)
//!             avg(x, window)  moving average over `window` seconds
//!             integ(x)        running integral since the start, x·s (integ(I): charge, C)
//!             ddt(x)          derivative, x/s
//! ```
//! Examples: `P*1000` (mW), `V/I` (Ω), `integ(I)/3600` (Ah), `avg(I, 1ms)`.
//!
//! Compiled once to a stack program; stateful functions (avg, integ, ddt)
//! keep their state per call site and start over at a gap.

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Const(f64),
    I,
    V,
    P,
    T,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Neg,
    Abs,
    Sqrt,
    Exp,
    Ln,
    Log10,
    Min,
    Max,
    /// moving average: state slot, window (s)
    Avg(usize, f64),
    Integ(usize),
    Ddt(usize),
}

#[derive(Debug, Clone)]
enum State {
    Avg { ring: Vec<f64>, pos: usize, filled: usize, sum: f64, n: usize },
    Integ(f64),
    Ddt(Option<f64>),
}

#[derive(Debug, Clone)]
pub struct MathProgram {
    pub source: String,
    ops: Vec<Op>,
    states: Vec<State>,
    /// the evaluation stack (depth checked when compiling)
    stack: [f64; STACK],
    /// it reads t (else the caller needn't work out each sample's time)
    pub uses_t: bool,
    dt: f64,
}

struct Parser<'a> {
    s: &'a [u8],
    p: usize,
    ops: Vec<Op>,
    states: Vec<State>,
}

fn prefix(c: char) -> Option<f64> {
    Some(match c {
        'p' => 1e-12,
        'n' => 1e-9,
        'u' | 'µ' => 1e-6,
        'm' => 1e-3,
        'k' => 1e3,
        'M' => 1e6,
        'G' => 1e9,
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.p < self.s.len() && (self.s[self.p] as char).is_whitespace() {
            self.p += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.ws();
        self.rest().chars().next()
    }

    fn rest(&self) -> &str {
        std::str::from_utf8(&self.s[self.p..]).unwrap_or("")
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.p += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn err<T>(&self, msg: &str) -> Result<T, String> {
        Err(format!("{msg} at position {}", self.p + 1))
    }

    fn expr(&mut self) -> Result<(), String> {
        self.term()?;
        loop {
            if self.eat('+') {
                self.term()?;
                self.ops.push(Op::Add);
            } else if self.eat('-') {
                self.term()?;
                self.ops.push(Op::Sub);
            } else {
                return Ok(());
            }
        }
    }

    fn term(&mut self) -> Result<(), String> {
        self.unary()?;
        loop {
            if self.eat('*') {
                self.unary()?;
                self.ops.push(Op::Mul);
            } else if self.eat('/') {
                self.unary()?;
                self.ops.push(Op::Div);
            } else {
                return Ok(());
            }
        }
    }

    fn unary(&mut self) -> Result<(), String> {
        if self.eat('-') {
            self.unary()?;
            self.ops.push(Op::Neg);
            return Ok(());
        }
        self.eat('+');
        self.power()
    }

    /// right-associative: 2^3^2 = 2^9
    fn power(&mut self) -> Result<(), String> {
        self.primary()?;
        if self.eat('^') {
            self.unary()?;
            self.ops.push(Op::Pow);
        }
        Ok(())
    }

    fn number(&mut self) -> Result<f64, String> {
        let r = self.rest();
        let mut end = 0;
        let b = r.as_bytes();
        while end < b.len() && (b[end].is_ascii_digit() || b[end] == b'.') {
            end += 1;
        }
        // exponent
        if end < b.len() && (b[end] == b'e' || b[end] == b'E') {
            let mut e = end + 1;
            if e < b.len() && (b[e] == b'+' || b[e] == b'-') {
                e += 1;
            }
            if e < b.len() && b[e].is_ascii_digit() {
                while e < b.len() && b[e].is_ascii_digit() {
                    e += 1;
                }
                end = e;
            }
        }
        let mut v: f64 = r[..end].parse().map_err(|_| format!("bad number at position {}", self.p + 1))?;
        self.p += end;
        // an SI prefix, then a unit's letters, are part of the number
        let r = self.rest();
        let mut chars = r.chars();
        if let Some(c) = chars.next() {
            if let Some(f) = prefix(c) {
                // a prefix only if a letter or a non-identifier follows (1m, 1ms; not 1max)
                let next = chars.clone().next();
                let unit_after = next.is_none_or(|n| !n.is_ascii_alphanumeric() || n == 's' || n == 'A' || n == 'V' || n == 'W' || n == 'J' || n == 'C');
                if unit_after {
                    v *= f;
                    self.p += c.len_utf8();
                }
            }
        }
        // a unit after the number: letters up to an operator
        while self.p < self.s.len() && (self.s[self.p] as char).is_ascii_alphabetic() {
            self.p += 1;
        }
        Ok(v)
    }

    fn primary(&mut self) -> Result<(), String> {
        match self.peek() {
            None => self.err("expression ends too soon"),
            Some('(') => {
                self.p += 1;
                self.expr()?;
                if !self.eat(')') {
                    return self.err("')' expected");
                }
                Ok(())
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let v = self.number()?;
                self.ops.push(Op::Const(v));
                Ok(())
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let start = self.p;
                while self.p < self.s.len() && ((self.s[self.p] as char).is_ascii_alphanumeric() || self.s[self.p] == b'_') {
                    self.p += 1;
                }
                let name = std::str::from_utf8(&self.s[start..self.p]).unwrap().to_string();
                if self.eat('(') {
                    return self.call(&name);
                }
                let op = match name.as_str() {
                    "I" | "i" => Op::I,
                    "V" | "v" => Op::V,
                    "P" | "p" => Op::P,
                    "t" => Op::T,
                    "pi" => Op::Const(std::f64::consts::PI),
                    _ => {
                        self.p = start;
                        return self.err(&format!("unknown name '{name}' (use I, V, P, t)"));
                    }
                };
                self.ops.push(op);
                Ok(())
            }
            Some(c) => self.err(&format!("unexpected '{c}'")),
        }
    }

    fn args(&mut self, n: usize, name: &str) -> Result<(), String> {
        for k in 0..n {
            if k > 0 && !self.eat(',') {
                return self.err(&format!("{name}() takes {n} arguments"));
            }
            self.expr()?;
        }
        if !self.eat(')') {
            return self.err(&format!("')' expected after {name}()'s arguments"));
        }
        Ok(())
    }

    fn call(&mut self, name: &str) -> Result<(), String> {
        let unary = |op| Some((1, op));
        let (n, op) = match name {
            "abs" => unary(Op::Abs),
            "sqrt" => unary(Op::Sqrt),
            "exp" => unary(Op::Exp),
            "ln" => unary(Op::Ln),
            "log10" | "log" => unary(Op::Log10),
            "min" => Some((2, Op::Min)),
            "max" => Some((2, Op::Max)),
            "integ" => {
                self.states.push(State::Integ(0.0));
                Some((1, Op::Integ(self.states.len() - 1)))
            }
            "ddt" => {
                self.states.push(State::Ddt(None));
                Some((1, Op::Ddt(self.states.len() - 1)))
            }
            "avg" => {
                // avg(x, window): the window must be a constant (s)
                self.expr()?;
                if !self.eat(',') {
                    return self.err("avg(x, window): a window in seconds, e.g. avg(I, 1ms)");
                }
                let mark = self.ops.len();
                self.expr()?;
                let w = match &self.ops[mark..] {
                    [Op::Const(w)] if *w > 0.0 => *w,
                    _ => return self.err("avg()'s window must be a positive number, e.g. 1ms"),
                };
                self.ops.truncate(mark);
                if !self.eat(')') {
                    return self.err("')' expected after avg()'s arguments");
                }
                self.states.push(State::Avg { ring: Vec::new(), pos: 0, filled: 0, sum: 0.0, n: 0 });
                self.ops.push(Op::Avg(self.states.len() - 1, w));
                return Ok(());
            }
            _ => None,
        }
        .ok_or_else(|| format!("unknown function '{name}' (abs sqrt exp ln log10 min max avg integ ddt)"))?;
        self.args(n, name)?;
        self.ops.push(op);
        Ok(())
    }
}

const STACK: usize = 32;

/// Moving averages longer than this many samples are refused (memory).
const MAX_AVG: usize = 1 << 22;

impl MathProgram {
    pub fn compile(src: &str) -> Result<MathProgram, String> {
        let mut p = Parser { s: src.as_bytes(), p: 0, ops: Vec::new(), states: Vec::new() };
        if src.trim().is_empty() {
            return Err("empty expression".into());
        }
        p.expr()?;
        if p.peek().is_some() {
            return p.err("unexpected text");
        }
        // how deep the stack gets: values push one, binary operators take one off
        let mut depth = 0i32;
        let mut max = 0;
        for op in &p.ops {
            depth += match op {
                Op::Const(_) | Op::I | Op::V | Op::P | Op::T => 1,
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Pow | Op::Min | Op::Max => -1,
                _ => 0,
            };
            max = max.max(depth);
        }
        if max as usize > STACK {
            return Err(format!("expression too deep (more than {STACK} values pending)"));
        }
        let uses_t = p.ops.contains(&Op::T);
        Ok(MathProgram { source: src.to_string(), ops: p.ops, states: p.states, stack: [0.0; STACK], uses_t, dt: 0.0 })
    }

    /// Start the stateful functions over (a gap, or a new pass over the data).
    pub fn reset(&mut self) {
        for s in &mut self.states {
            match s {
                State::Avg { ring, pos, filled, sum, .. } => {
                    // the ring too: its old values would be taken off the new sum
                    ring.fill(0.0);
                    *pos = 0;
                    *filled = 0;
                    *sum = 0.0;
                }
                State::Integ(_) => {}
                State::Ddt(prev) => *prev = None,
            }
        }
    }

    /// Integrals back to zero too.
    pub fn reset_all(&mut self) {
        self.reset();
        for s in &mut self.states {
            if let State::Integ(a) = s {
                *a = 0.0;
            }
        }
    }

    /// One sample: current, voltage, time, s to the next sample.
    #[inline]
    pub fn eval(&mut self, i: f64, v: f64, t: f64, dt: f64) -> f64 {
        if dt != self.dt {
            self.dt = dt;
            // windows in samples follow the sample period
            for (k, op) in self.ops.iter().enumerate() {
                if let Op::Avg(slot, w) = *op {
                    let n = ((w / dt).round() as usize).clamp(1, MAX_AVG);
                    self.states[slot] = State::Avg { ring: vec![0.0; n], pos: 0, filled: 0, sum: 0.0, n };
                    let _ = k;
                }
            }
        }
        // a fixed stack with an index: no allocation, no growth checks
        let st = &mut self.stack;
        let mut sp = 0usize;
        macro_rules! push {
            ($x:expr) => {{
                st[sp] = $x;
                sp += 1;
            }};
        }
        macro_rules! pop {
            () => {{
                sp -= 1;
                st[sp]
            }};
        }
        for op in &self.ops {
            match *op {
                Op::Const(c) => push!(c),
                Op::I => push!(i),
                Op::V => push!(v),
                Op::P => push!(i * v),
                Op::T => push!(t),
                Op::Neg => {
                    let a = pop!();
                    push!(-a)
                }
                Op::Abs | Op::Sqrt | Op::Exp | Op::Ln | Op::Log10 => {
                    let a = pop!();
                    push!(match op {
                        Op::Abs => a.abs(),
                        Op::Sqrt => a.sqrt(),
                        Op::Exp => a.exp(),
                        Op::Ln => a.ln(),
                        _ => a.log10(),
                    })
                }
                Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Pow | Op::Min | Op::Max => {
                    let b = pop!();
                    let a = pop!();
                    push!(match op {
                        Op::Add => a + b,
                        Op::Sub => a - b,
                        Op::Mul => a * b,
                        Op::Div => a / b,
                        Op::Pow => a.powf(b),
                        Op::Min => a.min(b),
                        _ => a.max(b),
                    })
                }
                Op::Avg(slot, _) => {
                    let x = pop!();
                    let State::Avg { ring, pos, filled, sum, n } = &mut self.states[slot] else { unreachable!() };
                    *sum += x - ring[*pos];
                    ring[*pos] = x;
                    *pos = (*pos + 1) % *n;
                    *filled = (*filled + 1).min(*n);
                    if *pos == 0 {
                        *sum = ring.iter().sum(); // drift
                    }
                    push!(*sum / *filled as f64)
                }
                Op::Integ(slot) => {
                    let x = pop!();
                    let State::Integ(acc) = &mut self.states[slot] else { unreachable!() };
                    if x.is_finite() {
                        *acc += x * dt;
                    }
                    push!(*acc)
                }
                Op::Ddt(slot) => {
                    let x = pop!();
                    let State::Ddt(prev) = &mut self.states[slot] else { unreachable!() };
                    let d = prev.map_or(0.0, |p| (x - p) / dt);
                    *prev = Some(x);
                    push!(d)
                }
            }
        }
        if sp == 0 {
            f64::NAN
        } else {
            pop!()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str, i: f64, v: f64) -> f64 {
        MathProgram::compile(src).unwrap().eval(i, v, 0.0, 1e-6)
    }

    #[test]
    fn arithmetic_precedence_units() {
        assert_eq!(ev("1 + 2 * 3", 0.0, 0.0), 7.0);
        assert_eq!(ev("(1 + 2) * 3", 0.0, 0.0), 9.0);
        assert_eq!(ev("-2^2", 0.0, 0.0), -4.0);
        assert_eq!(ev("2^3^2", 0.0, 0.0), 512.0);
        assert!((ev("P*1000", 0.002, 3.0) - 6.0).abs() < 1e-12);
        assert!((ev("V/I", 0.5, 3.0) - 6.0).abs() < 1e-12);
        assert!((ev("I/10m", 0.02, 0.0) - 2.0).abs() < 1e-12);
        assert!((ev("I + 1mA", 0.0, 0.0) - 1e-3).abs() < 1e-15);
        assert!((ev("max(I, 5u)", 1e-6, 0.0) - 5e-6).abs() < 1e-18);
        assert!((ev("sqrt(abs(-16))", 0.0, 0.0) - 4.0).abs() < 1e-12);
    }

    #[test]
    fn errors_say_where() {
        assert!(MathProgram::compile("I +").unwrap_err().contains("too soon"));
        assert!(MathProgram::compile("foo(I)").unwrap_err().contains("unknown function"));
        assert!(MathProgram::compile("X*2").unwrap_err().contains("unknown name"));
        assert!(MathProgram::compile("avg(I, I)").unwrap_err().contains("window"));
        assert!(MathProgram::compile("(I").unwrap_err().contains("')'"));
    }

    #[test]
    fn stateful_functions() {
        let mut m = MathProgram::compile("integ(I)").unwrap();
        let mut last = 0.0;
        for k in 0..1000 {
            last = m.eval(2.0, 0.0, k as f64 * 1e-3, 1e-3);
        }
        assert!((last - 2.0).abs() < 1e-9); // 2 A for 1 s
        let mut a = MathProgram::compile("avg(I, 4ms)").unwrap();
        let ys: Vec<f64> = (0..8).map(|k| a.eval(if k < 4 { 0.0 } else { 4.0 }, 0.0, 0.0, 1e-3)).collect();
        assert_eq!(ys[7], 4.0);
        assert_eq!(ys[5], 2.0);
        // after a reset (a gap) the average starts over, nothing old in it
        a.reset();
        assert_eq!(a.eval(1.0, 0.0, 0.0, 1e-3), 1.0);
        assert_eq!(a.eval(1.0, 0.0, 0.0, 1e-3), 1.0);
        let mut d = MathProgram::compile("ddt(I)").unwrap();
        d.eval(0.0, 0.0, 0.0, 1e-3);
        assert!((d.eval(1.0, 0.0, 0.0, 1e-3) - 1000.0).abs() < 1e-9);
    }
}
