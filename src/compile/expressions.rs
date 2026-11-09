use super::{SAGE_NS, escape_xml, has_content, invalid};
use roxmltree::Node;
use std::collections::HashMap;
use std::io;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;

pub(super) type Definitions = HashMap<Rc<str>, Definition>;

#[derive(Clone)]
pub(super) struct Definition {
    source: Rc<Path>,
    value: Rc<str>,
}

pub(super) fn merge_definitions(
    scope: &mut Rc<Definitions>,
    included: &Rc<Definitions>,
    path: &Path,
) -> io::Result<()> {
    if scope.is_empty() {
        *scope = Rc::clone(included);
    } else if !Rc::ptr_eq(scope, included) {
        for (name, definition) in included.iter() {
            if let Some(previous) = scope.get(name) {
                if previous.source != definition.source {
                    return Err(invalid(
                        path,
                        format!(
                            "define {name} is declared in {} and {}",
                            previous.source.display(),
                            definition.source.display(),
                        ),
                    ));
                }
            } else {
                Rc::make_mut(scope).insert(Rc::clone(name), definition.clone());
            }
        }
    }
    Ok(())
}

pub(super) fn read_definitions(
    node: Node<'_, '_>,
    scope: &mut Rc<Definitions>,
    path: &Path,
) -> io::Result<()> {
    if node.attributes().len() != 0 {
        return Err(invalid(path, "unexpected Defines attributes"));
    }
    let origin: Rc<Path> = Rc::from(path);
    for item in node.children().filter(Node::is_element) {
        let override_value = match item.attribute("override") {
            Some("true") => true,
            None | Some("false" | "0" | "1") => false,
            _ => return Err(invalid(path, "invalid Define override")),
        };
        let (Some(name), Some(value)) = (item.attribute("name"), item.attribute("value")) else {
            return Err(invalid(path, "Define requires a name and value"));
        };
        if !item.has_tag_name((SAGE_NS, "Define"))
            || item.attributes().len() != 2 + usize::from(item.attribute("override").is_some())
            || name.is_empty()
            || has_content(item)
        {
            return Err(invalid(path, "invalid Define element"));
        }
        if let Some(previous) = scope.get(name)
            && previous.source != origin
            && !override_value
        {
            return Err(invalid(
                path,
                format!(
                    "define {name} is already declared in {}",
                    previous.source.display(),
                ),
            ));
        }
        let value = if let Some(expression) = value.strip_prefix('=') {
            evaluate(expression, scope)
                .map_err(|error| invalid(path, format!("define {name}: {error}")))?
                .into()
        } else {
            Rc::from(value)
        };
        Rc::make_mut(scope).insert(
            Rc::from(name),
            Definition {
                source: Rc::clone(&origin),
                value,
            },
        );
    }
    Ok(())
}

pub(super) fn expand_assets(
    root: Node<'_, '_>,
    scope: &Definitions,
    path: &Path,
) -> io::Result<Option<Rc<str>>> {
    let document = root.document();
    let original = document.input_text();
    let mut xml = String::new();
    let mut copied = 0;
    let mut replace = |range: Range<usize>, value: &str, attribute: bool| -> io::Result<()> {
        let Some(expression) = value.strip_prefix('=') else {
            return Ok(());
        };
        let result = evaluate(expression, scope).map_err(|error| {
            let position = document.text_pos_at(range.start);
            invalid(path, format!("XML expression at {position}: {error}"))
        })?;
        xml.push_str(&original[copied..range.start]);
        if attribute && original.as_bytes()[range.start - 1] == b'\'' {
            let mut escaped = String::new();
            escape_xml(&result, true, &mut escaped);
            xml.push_str(&escaped.replace('\'', "&apos;"));
        } else {
            escape_xml(&result, attribute, &mut xml);
        }
        copied = range.end;
        Ok(())
    };
    for asset in root.children().filter(Node::is_element) {
        if ["Includes", "Tags", "Defines"]
            .iter()
            .any(|&name| asset.has_tag_name((SAGE_NS, name)))
        {
            continue;
        }
        for node in asset.descendants() {
            for attr in node.attributes() {
                replace(attr.range_value(), attr.value(), true)?;
            }
            if node.is_text() {
                replace(node.range(), node.text().unwrap_or_default(), false)?;
            }
        }
    }
    if copied == 0 {
        return Ok(None);
    }
    xml.push_str(&original[copied..]);
    Ok(Some(xml.into()))
}

fn evaluate(expression: &str, definitions: &Definitions) -> Result<String, String> {
    let mut parser = Parser {
        rest: expression,
        definitions,
    };
    let value = parser.binary(0, 0)?;
    if !parser.rest.trim().is_empty() {
        return Err(format!(
            "unexpected expression input {:?}",
            parser.rest.trim()
        ));
    }
    value.render()
}

enum Value {
    Integer(i32),
    Float(f64),
    Text(String),
}

impl Value {
    fn number(self) -> Result<f64, String> {
        match self {
            Self::Integer(value) => Ok(f64::from(value)),
            Self::Float(value) => Ok(value),
            Self::Text(value) => Err(format!("expected a number, found {value:?}")),
        }
    }

    fn render(self) -> Result<String, String> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Integer(value) => Ok(value.to_string()),
            Self::Float(value) if value.is_finite() => Ok(format_number(value)),
            Self::Float(_) => Err("expression result is not finite".to_owned()),
        }
    }
}

#[derive(Clone, Copy)]
enum Operator {
    Or,
    And,
    Equal,
    NotEqual,
    LessEqual,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

impl Operator {
    const TOKENS: [(&str, Self, u8); 11] = [
        ("or", Self::Or, 0),
        ("and", Self::And, 1),
        ("!=", Self::NotEqual, 2),
        ("=", Self::Equal, 2),
        ("<=", Self::LessEqual, 3),
        (">=", Self::GreaterEqual, 3),
        ("+", Self::Add, 4),
        ("-", Self::Subtract, 4),
        ("*", Self::Multiply, 5),
        ("/", Self::Divide, 5),
        ("%", Self::Remainder, 5),
    ];

    fn apply(self, left: Value, right: Value) -> Result<Value, String> {
        use Operator::*;
        use Value::*;

        if matches!(left, Text(_)) || matches!(right, Text(_)) {
            return match self {
                Add => Ok(Text(left.render()? + &right.render()?)),
                Equal | NotEqual => {
                    let equal = left.render()? == right.render()?;
                    Ok(Integer(i32::from(equal == matches!(self, Equal))))
                }
                _ => Err("operator requires numeric operands".to_owned()),
            };
        }
        if let (Integer(left), Integer(right)) = (&left, &right) {
            let (left, right) = (*left, *right);
            let result = match self {
                Or => i32::from(left != 0 || right != 0),
                And => i32::from(left != 0 && right != 0),
                Equal => i32::from(left == right),
                NotEqual => i32::from(left != right),
                LessEqual => i32::from(left <= right),
                GreaterEqual => i32::from(left >= right),
                Add => left.wrapping_add(right),
                Subtract => left.wrapping_sub(right),
                Multiply => left.wrapping_mul(right),
                Divide | Remainder => {
                    if right == 0 {
                        return Err("division by zero".to_owned());
                    }
                    let remainder = left
                        .checked_rem(right)
                        .ok_or_else(|| "integer division overflow".to_owned())?;
                    match self {
                        Remainder => remainder,
                        _ if remainder == 0 => left / right,
                        _ => return Ok(Float(f64::from(left) / f64::from(right))),
                    }
                }
            };
            return Ok(Integer(result));
        }
        let (left, right) = (left.number()?, right.number()?);
        match self {
            Or | And | Remainder => Err("operator requires integer operands".to_owned()),
            Equal => Ok(Integer(i32::from(left == right))),
            NotEqual => Ok(Integer(i32::from(left != right))),
            LessEqual => Ok(Integer(i32::from(left <= right))),
            GreaterEqual => Ok(Integer(i32::from(left >= right))),
            Add => Ok(Float(left + right)),
            Subtract => Ok(Float(left - right)),
            Multiply => Ok(Float(left * right)),
            Divide if right == 0.0 => Err("division by zero".to_owned()),
            Divide => Ok(Float(left / right)),
        }
    }
}

struct Parser<'a> {
    rest: &'a str,
    definitions: &'a Definitions,
}

impl Parser<'_> {
    fn take(&mut self, tokens: &str) -> Option<char> {
        self.rest = self.rest.trim_start();
        let next = self.rest.chars().next()?;
        if !tokens.contains(next) {
            return None;
        }
        self.rest = &self.rest[next.len_utf8()..];
        Some(next)
    }

    fn binary(&mut self, minimum: u8, depth: usize) -> Result<Value, String> {
        let mut result = self.term(depth)?;
        loop {
            self.rest = self.rest.trim_start();
            let Some((operator, precedence, rest)) =
                Operator::TOKENS
                    .iter()
                    .find_map(|&(token, operator, precedence)| {
                        Some((operator, precedence, self.rest.strip_prefix(token)?))
                    })
            else {
                return Ok(result);
            };
            if precedence < minimum {
                return Ok(result);
            }
            self.rest = rest;
            let right = self.binary(precedence + 1, depth)?;
            result = operator.apply(result, right)?;
        }
    }

    fn term(&mut self, depth: usize) -> Result<Value, String> {
        if depth >= 128 {
            return Err("expression is nested more than 128 levels".to_owned());
        }
        if let Some(sign) = self.take("+-") {
            return match self.term(depth + 1)? {
                Value::Integer(value) => Ok(Value::Integer(if sign == '-' {
                    value.wrapping_neg()
                } else {
                    value
                })),
                value => value
                    .number()
                    .map(|value| Value::Float(if sign == '-' { -value } else { value })),
            };
        }
        if let Some(rest) = self.rest.strip_prefix("round") {
            self.rest = rest;
            self.take("(")
                .ok_or_else(|| "expected '(' after round".to_owned())?;
            let number = self.group(depth)?.number()?.round_ties_even();
            let integer = if number >= f64::from(i32::MIN) && number <= f64::from(i32::MAX) {
                number as i32
            } else {
                i32::MIN
            };
            return Ok(Value::Integer(integer));
        }
        if self.take("(").is_some() {
            return self.group(depth);
        }
        if self.take("$").is_some() {
            let end = self
                .rest
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                .count();
            let (name, rest) = self.rest.split_at(end);
            self.rest = rest;
            let define = self
                .definitions
                .get(name)
                .ok_or_else(|| format!("unknown define ${name}"))?;
            let value = &define.value;
            return Ok(value
                .trim()
                .parse::<i32>()
                .map(Value::Integer)
                .or_else(|_| value.trim().parse::<f64>().map(Value::Float))
                .unwrap_or_else(|_| Value::Text(value.to_string())));
        }
        if let Some(quote) = self.take("'\"") {
            let (text, rest) = self
                .rest
                .split_once(quote)
                .ok_or_else(|| "unterminated string literal".to_owned())?;
            self.rest = rest;
            return Ok(Value::Text(text.to_owned()));
        }
        let end = self
            .rest
            .bytes()
            .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
            .count();
        let (number, rest) = self.rest.split_at(end);
        self.rest = rest;
        if number.contains('.') {
            number
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| "invalid decimal literal".to_owned())
        } else {
            number
                .parse::<i32>()
                .map(Value::Integer)
                .map_err(|_| "expected a 32-bit integer, define, or string literal".to_owned())
        }
    }

    fn group(&mut self, depth: usize) -> Result<Value, String> {
        let result = self.binary(0, depth + 1)?;
        self.take(")")
            .ok_or_else(|| "missing closing parenthesis".to_owned())?;
        Ok(result)
    }
}

fn format_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    // The SDK uses .NET Framework's invariant general format for doubles.
    let scientific = format!("{value:.14e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("scientific notation");
    let exponent: i32 = exponent.parse().expect("decimal exponent");
    if (-4..15).contains(&exponent) {
        let fixed = format!("{value:.precision$}", precision = (14 - exponent) as usize);
        if fixed.contains('.') {
            return fixed.trim_end_matches('0').trim_end_matches('.').to_owned();
        }
        fixed
    } else {
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        format!("{mantissa}E{exponent:+03}")
    }
}
