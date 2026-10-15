//! Conditions written as text, for state machines and the like: `Health.value <= 0`, `$alert >= 1 and CharacterBody.grounded`, `$a == 1 or $b == 1`.
//!
//! An operand is a number, `true` or `false`, `$name` (a world resource, 0 when it does not exist) or `Component.field[i]` of the entity the
//! condition is asked about (0 when the entity lacks the component). Operators are `== != < <= > >=`; `and` binds tighter than `or`; an operand on
//! its own means "is not 0".
use crate::fieldio;
use lunaris_ecs::{Entity, World};

#[derive(Clone, Debug, PartialEq)]
enum Operand {
    Num(f64),
    Res(String),
    Field(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug, PartialEq)]
struct Test {
    left: Operand,
    op: Option<(Cmp, Operand)>,
}

/// A parsed condition: any of several groups, each group needing all of its tests.
#[derive(Clone, Debug, PartialEq)]
pub struct Cond {
    any: Vec<Vec<Test>>,
}

fn operand(word: &str) -> Result<Operand, String> {
    if let Some(name) = word.strip_prefix('$') {
        if name.is_empty() {
            return Err("`$` must be followed by a resource name".to_string());
        }
        return Ok(Operand::Res(name.to_string()));
    }
    match word {
        "true" => return Ok(Operand::Num(1.0)),
        "false" => return Ok(Operand::Num(0.0)),
        _ => {}
    }
    if let Ok(n) = word.parse::<f64>() {
        return if n.is_finite() { Ok(Operand::Num(n)) } else { Err(format!("`{word}` is not a finite number")) };
    }
    if word.contains('.') && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']')) {
        return Ok(Operand::Field(word.to_string()));
    }
    Err(format!("`{word}` is not a number, a $resource or a Component.field"))
}

/// Cut a condition into words and operators: `a<=3` and `a <= 3` are the same.
fn words(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let two = chars.get(i + 1).copied();
        let op = match (c, two) {
            ('=', Some('=')) | ('!', Some('=')) | ('<', Some('=')) | ('>', Some('=')) => Some(2),
            ('<', _) | ('>', _) => Some(1),
            _ => None,
        };
        if let Some(n) = op {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(chars[i..i + n].iter().collect());
            i += n;
        } else if c.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            i += 1;
        } else {
            cur.push(c);
            i += 1;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

impl Cond {
    pub fn parse(src: &str) -> Result<Cond, String> {
        let ws = words(src);
        if ws.is_empty() {
            return Err("an empty condition".to_string());
        }
        let mut any: Vec<Vec<Test>> = vec![Vec::new()];
        let mut i = 0;
        while i < ws.len() {
            let left = operand(&ws[i])?;
            i += 1;
            let mut test = Test { left, op: None };
            if let Some(op) = ws.get(i).and_then(|w| match w.as_str() {
                "==" => Some(Cmp::Eq),
                "!=" => Some(Cmp::Ne),
                "<" => Some(Cmp::Lt),
                "<=" => Some(Cmp::Le),
                ">" => Some(Cmp::Gt),
                ">=" => Some(Cmp::Ge),
                _ => None,
            }) {
                let right = ws.get(i + 1).ok_or_else(|| format!("`{}` needs a value after it", ws[i]))?;
                test.op = Some((op, operand(right)?));
                i += 2;
            }
            any.last_mut().expect("one group").push(test);
            match ws.get(i).map(|w| w.as_str()) {
                None => break,
                Some("and") => i += 1,
                Some("or") => {
                    any.push(Vec::new());
                    i += 1;
                }
                Some(other) => return Err(format!("expected `and`, `or` or the end after a test, found `{other}`")),
            }
            if i >= ws.len() {
                return Err("the condition ends after `and` / `or`".to_string());
            }
        }
        Ok(Cond { any })
    }

    /// The resources and field paths the condition reads (to check them without a world to ask).
    pub fn paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in self.any.iter().flatten() {
            for o in std::iter::once(&t.left).chain(t.op.iter().map(|(_, r)| r)) {
                if let Operand::Field(p) = o {
                    out.push(p.clone());
                }
            }
        }
        out
    }

    /// Is it true for entity `e`? A path that does not exist (a misspelt component or field) is an error, so it is not silently "false".
    pub fn eval(&self, world: &World, e: Entity) -> Result<bool, String> {
        let value = |o: &Operand| -> Result<f64, String> {
            Ok(match o {
                Operand::Num(n) => *n,
                Operand::Res(r) => world.resource(r).unwrap_or(0.0),
                Operand::Field(p) => {
                    let slot = fieldio::slot(world, p)?;
                    fieldio::read(world, e, &slot).unwrap_or(0.0)
                }
            })
        };
        for group in &self.any {
            let mut all = true;
            for t in group {
                let l = value(&t.left)?;
                let ok = match &t.op {
                    None => l != 0.0,
                    Some((op, r)) => {
                        let r = value(r)?;
                        match op {
                            Cmp::Eq => l == r,
                            Cmp::Ne => l != r,
                            Cmp::Lt => l < r,
                            Cmp::Le => l <= r,
                            Cmp::Gt => l > r,
                            Cmp::Ge => l >= r,
                        }
                    }
                };
                if !ok {
                    all = false;
                    break;
                }
            }
            if all {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_parse_and_report_what_is_wrong() {
        assert!(Cond::parse("Health.value <= 0").is_ok());
        assert!(Cond::parse("$a>=1 and $b<3 or true").is_ok());
        assert!(Cond::parse("CharacterBody.walk[0] != 0").is_ok());
        for bad in ["", "and", "$a >", "$a == 1 and", "$a 1", "foo", "$", "Health.value == == 1"] {
            assert!(Cond::parse(bad).is_err(), "{bad}");
        }
        let c = Cond::parse("A.b == 1 and $x > A.c or Q.r[2]").unwrap();
        assert_eq!(c.paths(), vec!["A.b", "A.c", "Q.r[2]"]);
    }
}
