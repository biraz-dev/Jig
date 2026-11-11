//! Bounded spreadsheet formula parsing and reference-only editing.
//! Evaluation and dependency tracking belong to the workbook, not this parser.
use crate::spreadsheet::{MAX_COLS, MAX_ROWS};
use std::fmt;

const MAX_BYTES: usize = 4096;
const MAX_TOKENS: usize = 4096;
const MAX_DEPTH: usize = 128;
const MAX_ARGUMENTS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormulaError {
    Parse,
    Ref,
    Name,
    Value,
    DivZero,
    Num,
    Cycle,
    Limit,
}

impl FormulaError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Parse => "#ERROR!",
            Self::Ref => "#REF!",
            Self::Name => "#NAME?",
            Self::Value => "#VALUE!",
            Self::DivZero => "#DIV/0!",
            Self::Num => "#NUM!",
            Self::Cycle => "#CYCLE!",
            Self::Limit => "#LIMIT!",
        }
    }
}

impl fmt::Display for FormulaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for FormulaError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    pub sheet: Option<String>,
    /// Zero-based row and column, as used by the workbook and grid.
    pub row: usize,
    pub col: usize,
    pub row_absolute: bool,
    pub col_absolute: bool,
}

impl Reference {
    fn within_bounds(&self) -> bool {
        self.row < MAX_ROWS && self.col < MAX_COLS
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Plus,
    Minus,
    Percent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(FormulaError),
    Ref(Reference),
    Range(Reference, Reference),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Clone, Debug)]
struct ReferenceToken {
    reference: Reference,
    cell_start: usize,
    cell_end: usize,
    sheet_span: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
enum Kind {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(FormulaError),
    Ref(ReferenceToken),
    Name(String),
    Plus,
    Minus,
    Star,
    Slash,
    Power,
    Percent,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Left,
    Right,
    Comma,
    Colon,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

struct Lexer<'a> {
    input: &'a str,
    position: usize,
}

impl Lexer<'_> {
    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }

    fn space(&mut self) {
        while let Some(character) = self.input[self.position..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.position += character.len_utf8();
        }
    }

    fn quoted(&mut self, quote: u8) -> Result<String, FormulaError> {
        self.position += 1;
        let mut value = String::new();
        loop {
            match self.byte() {
                Some(byte) if byte == quote => {
                    self.position += 1;
                    if self.byte() == Some(quote) {
                        value.push(char::from(quote));
                        self.position += 1;
                    } else {
                        return Ok(value);
                    }
                }
                Some(_) => {
                    let character = self.input[self.position..]
                        .chars()
                        .next()
                        .ok_or(FormulaError::Parse)?;
                    value.push(character);
                    self.position += character.len_utf8();
                }
                None => return Err(FormulaError::Parse),
            }
        }
    }

    fn number(&mut self) -> Result<Kind, FormulaError> {
        let start = self.position;
        while self.byte().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        if self.byte() == Some(b'.') {
            self.position += 1;
            while self.byte().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        if self.byte().is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            self.position += 1;
            if self.byte().is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                self.position += 1;
            }
            let exponent = self.position;
            while self.byte().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if self.position == exponent {
                return Err(FormulaError::Parse);
            }
        }
        let number = self.input[start..self.position]
            .parse::<f64>()
            .map_err(|_| FormulaError::Parse)?;
        if !number.is_finite() {
            return Err(FormulaError::Num);
        }
        Ok(Kind::Number(number))
    }

    fn qualified(
        &mut self,
        sheet: String,
        sheet_span: (usize, usize),
    ) -> Result<Kind, FormulaError> {
        self.position += 1; // !
        self.space();
        let cell_start = self.position;
        while self
            .byte()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'$')
        {
            self.position += 1;
        }
        let reference =
            reference(&self.input[cell_start..self.position]).ok_or(FormulaError::Ref)?;
        if sheet.is_empty() {
            return Err(FormulaError::Ref);
        }
        Ok(Kind::Ref(ReferenceToken {
            reference: Reference {
                sheet: Some(sheet),
                ..reference
            },
            cell_start,
            cell_end: self.position,
            sheet_span: Some(sheet_span),
        }))
    }

    fn next(&mut self) -> Result<Token, FormulaError> {
        self.space();
        let start = self.position;
        let kind = match self.byte() {
            None => Kind::End,
            Some(b'0'..=b'9' | b'.') => self.number()?,
            Some(b'"') => Kind::Text(self.quoted(b'"')?),
            Some(b'\'') => {
                let sheet = self.quoted(b'\'')?;
                let end = self.position;
                self.space();
                if self.byte() != Some(b'!') {
                    return Err(FormulaError::Parse);
                }
                self.qualified(sheet, (start, end))?
            }
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'_' | b'$') => {
                while self.byte().is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$')
                }) {
                    self.position += 1;
                }
                let end = self.position;
                let input = self.input;
                let word = &input[start..end];
                self.space();
                if self.byte() == Some(b'!') {
                    self.qualified(word.to_owned(), (start, end))?
                } else if self.byte() == Some(b'(') {
                    Kind::Name(word.to_ascii_uppercase())
                } else if word.eq_ignore_ascii_case("TRUE") {
                    Kind::Bool(true)
                } else if word.eq_ignore_ascii_case("FALSE") {
                    Kind::Bool(false)
                } else if let Some(reference) = reference(word) {
                    Kind::Ref(ReferenceToken {
                        reference,
                        cell_start: start,
                        cell_end: end,
                        sheet_span: None,
                    })
                } else {
                    Kind::Name(word.to_ascii_uppercase())
                }
            }
            Some(b'#') => {
                let errors = [
                    FormulaError::DivZero,
                    FormulaError::Value,
                    FormulaError::Cycle,
                    FormulaError::Limit,
                    FormulaError::Parse,
                    FormulaError::Name,
                    FormulaError::Ref,
                    FormulaError::Num,
                ];
                let error = errors
                    .into_iter()
                    .find(|error| {
                        self.input[self.position..]
                            .get(..error.code().len())
                            .is_some_and(|text| text.eq_ignore_ascii_case(error.code()))
                    })
                    .ok_or(FormulaError::Parse)?;
                self.position += error.code().len();
                Kind::Error(error)
            }
            Some(byte) => {
                self.position += 1;
                match byte {
                    b'+' => Kind::Plus,
                    b'-' => Kind::Minus,
                    b'*' => Kind::Star,
                    b'/' => Kind::Slash,
                    b'^' => Kind::Power,
                    b'%' => Kind::Percent,
                    b'=' => Kind::Equal,
                    b'(' => Kind::Left,
                    b')' => Kind::Right,
                    b',' | b';' => Kind::Comma,
                    b':' => Kind::Colon,
                    b'<' if self.byte() == Some(b'=') => {
                        self.position += 1;
                        Kind::LessEqual
                    }
                    b'<' if self.byte() == Some(b'>') => {
                        self.position += 1;
                        Kind::NotEqual
                    }
                    b'>' if self.byte() == Some(b'=') => {
                        self.position += 1;
                        Kind::GreaterEqual
                    }
                    b'<' => Kind::Less,
                    b'>' => Kind::Greater,
                    _ => return Err(FormulaError::Parse),
                }
            }
        };
        Ok(Token {
            kind,
            start,
            end: self.position,
        })
    }
}

fn reference(text: &str) -> Option<Reference> {
    let bytes = text.as_bytes();
    let mut position = 0;
    let col_absolute = bytes.first() == Some(&b'$');
    position += usize::from(col_absolute);
    let column_start = position;
    while bytes.get(position).is_some_and(u8::is_ascii_alphabetic) {
        position += 1;
    }
    if position == column_start {
        return None;
    }
    let row_absolute = bytes.get(position) == Some(&b'$');
    position += usize::from(row_absolute);
    let row_start = position;
    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    if row_start == position || position != bytes.len() {
        return None;
    }
    let column_end = row_start - usize::from(row_absolute);
    // Saturation gives every platform the same bounded invalid-reference node,
    // even for a 4000-digit coordinate. Inactive IF branches can then remain
    // lazy instead of turning a 32-bit integer parse failure into a global error.
    let column = bytes[column_start..column_end]
        .iter()
        .fold(0_usize, |column, byte| {
            column
                .saturating_mul(26)
                .saturating_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1))
                .min(MAX_COLS + 1)
        });
    let row_number = bytes[row_start..position]
        .iter()
        .fold(0_usize, |row, byte| {
            row.saturating_mul(10)
                .saturating_add(usize::from(byte - b'0'))
                .min(MAX_ROWS + 1)
        });
    let row = if row_number == 0 {
        MAX_ROWS
    } else {
        row_number - 1
    };
    Some(Reference {
        sheet: None,
        row,
        col: column - 1,
        row_absolute,
        col_absolute,
    })
}

fn tokens(input: &str) -> Result<Vec<Token>, FormulaError> {
    if input.len() > MAX_BYTES {
        return Err(FormulaError::Limit);
    }
    let mut lexer = Lexer { input, position: 0 };
    lexer.space();
    if lexer.byte() == Some(b'=') {
        lexer.position += 1;
    }
    let mut result = Vec::new();
    loop {
        if result.len() >= MAX_TOKENS {
            return Err(FormulaError::Limit);
        }
        let token = lexer.next()?;
        let end = matches!(token.kind, Kind::End);
        result.push(token);
        if end {
            return Ok(result);
        }
    }
}

struct Node {
    expression: Expr,
    height: usize,
}

impl Node {
    fn leaf(expression: Expr) -> Self {
        Self {
            expression,
            height: 1,
        }
    }

    fn with_height(expression: Expr, height: usize) -> Result<Self, FormulaError> {
        if height > MAX_DEPTH {
            return Err(FormulaError::Limit);
        }
        Ok(Self { expression, height })
    }
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> &Kind {
        &self.tokens[self.position].kind
    }

    fn take(&mut self) -> Kind {
        let kind = self.tokens[self.position].kind.clone();
        if !matches!(kind, Kind::End) {
            self.position += 1;
        }
        kind
    }

    fn range(&mut self, mut left: Reference) -> Result<Expr, FormulaError> {
        if !matches!(self.peek(), Kind::Colon) {
            return Ok(if left.within_bounds() {
                Expr::Ref(left)
            } else {
                Expr::Error(FormulaError::Ref)
            });
        }
        self.take();
        let mut right = match self.take() {
            Kind::Ref(token) => token.reference,
            Kind::Error(error) => return Ok(Expr::Error(error)),
            _ => return Err(FormulaError::Parse),
        };
        match (&left.sheet, &right.sheet) {
            (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b) => {
                return Ok(Expr::Error(FormulaError::Ref));
            }
            (Some(sheet), None) => right.sheet = Some(sheet.clone()),
            (None, Some(sheet)) => left.sheet = Some(sheet.clone()),
            _ => {}
        }
        Ok(if left.within_bounds() && right.within_bounds() {
            Expr::Range(left, right)
        } else {
            Expr::Error(FormulaError::Ref)
        })
    }

    fn expression(&mut self, minimum: u8, depth: usize) -> Result<Node, FormulaError> {
        if depth > MAX_DEPTH {
            return Err(FormulaError::Limit);
        }
        let mut left = match self.take() {
            Kind::Number(number) => Node::leaf(Expr::Number(number)),
            Kind::Text(text) => Node::leaf(Expr::Text(text)),
            Kind::Bool(value) => Node::leaf(Expr::Bool(value)),
            Kind::Error(error) => {
                if matches!(self.peek(), Kind::Colon) {
                    self.take();
                    if !matches!(self.take(), Kind::Ref(_) | Kind::Error(_)) {
                        return Err(FormulaError::Parse);
                    }
                }
                Node::leaf(Expr::Error(error))
            }
            Kind::Ref(token) => Node::leaf(self.range(token.reference)?),
            Kind::Plus | Kind::Minus => {
                let operator = if matches!(self.tokens[self.position - 1].kind, Kind::Plus) {
                    UnaryOp::Plus
                } else {
                    UnaryOp::Minus
                };
                let argument = self.expression(4, depth + 1)?;
                Node::with_height(
                    Expr::Unary(operator, Box::new(argument.expression)),
                    argument.height + 1,
                )?
            }
            Kind::Left => {
                let expression = self.expression(0, depth + 1)?;
                if !matches!(self.take(), Kind::Right) {
                    return Err(FormulaError::Parse);
                }
                expression
            }
            Kind::Name(name) => {
                if !matches!(self.take(), Kind::Left) {
                    return Err(FormulaError::Name);
                }
                let mut arguments = Vec::new();
                let mut height = 0;
                if !matches!(self.peek(), Kind::Right) {
                    loop {
                        if arguments.len() >= MAX_ARGUMENTS {
                            return Err(FormulaError::Limit);
                        }
                        let argument = self.expression(0, depth + 1)?;
                        height = height.max(argument.height);
                        arguments.push(argument.expression);
                        if !matches!(self.peek(), Kind::Comma) {
                            break;
                        }
                        self.take();
                    }
                }
                if !matches!(self.take(), Kind::Right) {
                    return Err(FormulaError::Parse);
                }
                Node::with_height(Expr::Call(name, arguments), height + 1)?
            }
            _ => return Err(FormulaError::Parse),
        };
        loop {
            if matches!(self.peek(), Kind::Percent) {
                self.take();
                left = Node::with_height(
                    Expr::Unary(UnaryOp::Percent, Box::new(left.expression)),
                    left.height + 1,
                )?;
                continue;
            }
            let (operator, precedence) = match self.peek() {
                Kind::Equal => (BinaryOp::Equal, 1),
                Kind::NotEqual => (BinaryOp::NotEqual, 1),
                Kind::Less => (BinaryOp::Less, 1),
                Kind::LessEqual => (BinaryOp::LessEqual, 1),
                Kind::Greater => (BinaryOp::Greater, 1),
                Kind::GreaterEqual => (BinaryOp::GreaterEqual, 1),
                Kind::Plus => (BinaryOp::Add, 2),
                Kind::Minus => (BinaryOp::Subtract, 2),
                Kind::Star => (BinaryOp::Multiply, 3),
                Kind::Slash => (BinaryOp::Divide, 3),
                Kind::Power => (BinaryOp::Power, 5),
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.take();
            let right = self.expression(
                precedence + u8::from(operator != BinaryOp::Power),
                depth + 1,
            )?;
            let height = left.height.max(right.height) + 1;
            left = Node::with_height(
                Expr::Binary(
                    operator,
                    Box::new(left.expression),
                    Box::new(right.expression),
                ),
                height,
            )?;
        }
        Ok(left)
    }
}

/// Parse a formula without executing code or resolving any cells.
pub fn parse(input: &str) -> Result<Expr, FormulaError> {
    let mut parser = Parser {
        tokens: tokens(input)?,
        position: 0,
    };
    let expression = parser.expression(0, 0)?;
    if !matches!(parser.peek(), Kind::End) {
        return Err(FormulaError::Parse);
    }
    Ok(expression.expression)
}

fn edit(input: &str, replacements: Vec<(usize, usize, String)>) -> String {
    let mut output = String::with_capacity(input.len());
    let mut position = 0;
    for (start, end, value) in replacements {
        output.push_str(&input[position..start]);
        output.push_str(&value);
        position = end;
    }
    output.push_str(&input[position..]);
    output
}

fn column_label(mut column: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(char::from(b'A' + (column % 26) as u8));
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    letters.into_iter().rev().collect()
}

fn reference_label(reference: &Reference, row: usize, col: usize) -> String {
    format!(
        "{}{}{}{}",
        if reference.col_absolute { "$" } else { "" },
        column_label(col),
        if reference.row_absolute { "$" } else { "" },
        row + 1
    )
}

/// Move relative references for copy/fill. Literal text, quoted sheet names,
/// whitespace and absolute row/column anchors are preserved.
pub fn translate(input: &str, delta_row: i32, delta_col: i32) -> String {
    if !input.trim_start().starts_with('=') || (delta_row == 0 && delta_col == 0) {
        return input.to_owned();
    }
    let Ok(tokens) = tokens(input) else {
        return input.to_owned();
    };
    let mut replacements = Vec::new();
    for token in tokens {
        let Kind::Ref(reference) = token.kind else {
            continue;
        };
        let original = &reference.reference;
        if !original.within_bounds() {
            replacements.push((token.start, token.end, FormulaError::Ref.code().into()));
            continue;
        }
        let row = original.row as i64
            + if original.row_absolute {
                0
            } else {
                i64::from(delta_row)
            };
        let col = original.col as i64
            + if original.col_absolute {
                0
            } else {
                i64::from(delta_col)
            };
        if row < 0 || col < 0 || row >= MAX_ROWS as i64 || col >= MAX_COLS as i64 {
            replacements.push((token.start, token.end, FormulaError::Ref.code().into()));
        } else {
            let moved = reference_label(original, row as usize, col as usize);
            replacements.push((reference.cell_start, reference.cell_end, moved));
        }
    }
    edit(input, replacements)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    Row,
    Column,
}

impl Axis {
    fn limit(self) -> usize {
        match self {
            Self::Row => MAX_ROWS,
            Self::Column => MAX_COLS,
        }
    }

    fn coordinate(self, reference: &Reference) -> usize {
        match self {
            Self::Row => reference.row,
            Self::Column => reference.col,
        }
    }

    fn move_to(self, reference: &Reference, coordinate: usize) -> String {
        match self {
            Self::Row => reference_label(reference, coordinate, reference.col),
            Self::Column => reference_label(reference, reference.row, coordinate),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edit {
    Insert { at: usize, count: usize },
    Delete { at: usize, count: usize },
}

impl Edit {
    fn valid(self, limit: usize) -> bool {
        let (Self::Insert { at, count } | Self::Delete { at, count }) = self;
        count > 0 && at < limit && count <= limit - at
    }

    fn coordinate(self, coordinate: usize, limit: usize) -> Option<usize> {
        match self {
            Self::Insert { at, count } if coordinate >= at => {
                coordinate.checked_add(count).filter(|value| *value < limit)
            }
            Self::Delete { at, count } if coordinate >= at => {
                if coordinate < at + count {
                    None
                } else {
                    Some(coordinate - count)
                }
            }
            _ => Some(coordinate),
        }
    }

    fn range(self, first: usize, last: usize, limit: usize) -> Option<(usize, usize)> {
        if matches!(self, Self::Insert { .. }) {
            return Some((
                self.coordinate(first, limit)?,
                self.coordinate(last, limit)?,
            ));
        }
        let Self::Delete { at, count } = self else {
            return None;
        };
        let low = first.min(last);
        let high = first.max(last);
        let end = at + count;
        let removed_low = low.max(at);
        let removed_end = (high + 1).min(end);
        let removed = removed_end.saturating_sub(removed_low);
        let remaining = (high - low + 1).checked_sub(removed)?;
        if remaining == 0 {
            return None;
        }
        let low = if low >= end {
            low - count
        } else if low >= at {
            at
        } else {
            low
        };
        let high = low + remaining - 1;
        Some(if first <= last {
            (low, high)
        } else {
            (high, low)
        })
    }
}

fn names_match(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

/// Adjust references after a worksheet inserts or deletes rows or columns.
/// Structural edits move absolute references too: `$` constrains copy/fill,
/// while row/column insertion must continue referring to the original cells.
/// The referrer's sheet supplies the scope of unqualified references.
pub fn adjust_references(
    input: &str,
    referrer_sheet: &str,
    affected_sheet: &str,
    axis: Axis,
    change: Edit,
) -> String {
    if !input.trim_start().starts_with('=') || !change.valid(axis.limit()) {
        return input.to_owned();
    }
    let Ok(tokens) = tokens(input) else {
        return input.to_owned();
    };
    let mut replacements = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let Kind::Ref(left) = &token.kind else {
            index += 1;
            continue;
        };
        if let (Some(colon), Some(right_token)) = (tokens.get(index + 1), tokens.get(index + 2)) {
            if let (Kind::Colon, Kind::Ref(right)) = (&colon.kind, &right_token.kind) {
                let first = &left.reference;
                let last = &right.reference;
                let sheet = first
                    .sheet
                    .as_deref()
                    .or(last.sheet.as_deref())
                    .unwrap_or(referrer_sheet);
                let same_sheet = match (&first.sheet, &last.sheet) {
                    (Some(left), Some(right)) => names_match(left, right),
                    _ => true,
                };
                if same_sheet && names_match(sheet, affected_sheet) {
                    let moved = if first.within_bounds() && last.within_bounds() {
                        change.range(axis.coordinate(first), axis.coordinate(last), axis.limit())
                    } else {
                        None
                    };
                    if let Some((first_coordinate, last_coordinate)) = moved {
                        if first_coordinate != axis.coordinate(first) {
                            replacements.push((
                                left.cell_start,
                                left.cell_end,
                                axis.move_to(first, first_coordinate),
                            ));
                        }
                        if last_coordinate != axis.coordinate(last) {
                            replacements.push((
                                right.cell_start,
                                right.cell_end,
                                axis.move_to(last, last_coordinate),
                            ));
                        }
                    } else {
                        replacements.push((
                            token.start,
                            right.cell_end,
                            FormulaError::Ref.code().into(),
                        ));
                    }
                }
                index += 3;
                continue;
            }
        }
        let reference = &left.reference;
        let sheet = reference.sheet.as_deref().unwrap_or(referrer_sheet);
        if names_match(sheet, affected_sheet) {
            let moved = if reference.within_bounds() {
                change.coordinate(axis.coordinate(reference), axis.limit())
            } else {
                None
            };
            if let Some(coordinate) = moved {
                if coordinate != axis.coordinate(reference) {
                    replacements.push((
                        left.cell_start,
                        left.cell_end,
                        axis.move_to(reference, coordinate),
                    ));
                }
            } else {
                replacements.push((token.start, left.cell_end, FormulaError::Ref.code().into()));
            }
        }
        index += 1;
    }
    edit(input, replacements)
}

/// Update only explicit sheet qualifiers, including both ends of ranges.
pub fn rename_sheet_references(input: &str, old: &str, new: &str) -> String {
    if !input.trim_start().starts_with('=') {
        return input.to_owned();
    }
    let Ok(tokens) = tokens(input) else {
        return input.to_owned();
    };
    let mut replacements = Vec::new();
    for token in tokens {
        if let Kind::Ref(reference) = token.kind {
            if reference
                .reference
                .sheet
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(old))
            {
                if let Some((start, end)) = reference.sheet_span {
                    replacements.push((start, end, format!("'{}'", new.replace('\'', "''"))));
                }
            }
        }
    }
    edit(input, replacements)
}

/// Deleted sheet references become permanent #REF! errors, so later creating a
/// sheet with the old name cannot silently reconnect an unrelated formula.
pub fn delete_sheet_references(input: &str, sheet: &str) -> String {
    if !input.trim_start().starts_with('=') {
        return input.to_owned();
    }
    let Ok(tokens) = tokens(input) else {
        return input.to_owned();
    };
    let points_to_sheet = |token: &Token| match &token.kind {
        Kind::Ref(reference) => reference
            .reference
            .sheet
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(sheet)),
        _ => false,
    };
    let mut replacements = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if matches!(token.kind, Kind::Ref(_))
            && tokens
                .get(index + 1)
                .is_some_and(|token| matches!(token.kind, Kind::Colon))
            && tokens
                .get(index + 2)
                .is_some_and(|token| matches!(token.kind, Kind::Ref(_)))
        {
            let right = &tokens[index + 2];
            if points_to_sheet(token) || points_to_sheet(right) {
                replacements.push((token.start, right.end, FormulaError::Ref.code().into()));
            }
            index += 3;
        } else {
            if points_to_sheet(token) {
                replacements.push((token.start, token.end, FormulaError::Ref.code().into()));
            }
            index += 1;
        }
    }
    edit(input, replacements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_expr(sheet: Option<&str>, row: usize, col: usize) -> Expr {
        Expr::Ref(Reference {
            sheet: sheet.map(str::to_owned),
            row,
            col,
            row_absolute: false,
            col_absolute: false,
        })
    }

    #[test]
    fn arithmetic_precedence_and_power_are_explicit() {
        assert_eq!(
            parse("=-2^2+50%"),
            Ok(Expr::Binary(
                BinaryOp::Add,
                Box::new(Expr::Unary(
                    UnaryOp::Minus,
                    Box::new(Expr::Binary(
                        BinaryOp::Power,
                        Box::new(Expr::Number(2.0)),
                        Box::new(Expr::Number(2.0)),
                    )),
                )),
                Box::new(Expr::Unary(UnaryOp::Percent, Box::new(Expr::Number(50.0)))),
            ))
        );
        assert!(matches!(
            parse("=2^3^2"),
            Ok(Expr::Binary(BinaryOp::Power, _, right))
                if matches!(*right, Expr::Binary(BinaryOp::Power, _, _))
        ));
        assert_eq!(parse("=1.25e-2"), Ok(Expr::Number(0.0125)));
        assert_eq!(parse("=1e9999"), Err(FormulaError::Num));
    }

    #[test]
    fn sheet_names_ranges_strings_and_conditionals_parse_without_rewriting_literals() {
        let expression = parse(
            "=IF('Clay ''104'''!$A1>=94,SUM('Clay ''104'''!A1:B2),\"A1 says \"\"ready\"\"\")",
        );
        assert!(
            matches!(expression, Ok(Expr::Call(name, args)) if name == "IF" && args.len() == 3)
        );
        assert_eq!(
            parse("=Sheet1!A1"),
            Ok(reference_expr(Some("Sheet1"), 0, 0))
        );
        let Ok(Expr::Range(left, right)) = parse("='Clay tests'!A1:B2") else {
            panic!("expected a range");
        };
        assert_eq!(left.sheet.as_deref(), Some("Clay tests"));
        assert_eq!(right.sheet, left.sheet);
        assert_eq!((right.row, right.col), (1, 1));
        assert_eq!(parse("=TRUE"), Ok(Expr::Bool(true)));
        assert_eq!(parse("=unknown_name"), Err(FormulaError::Name));
    }

    #[test]
    fn copy_and_fill_shift_relative_references_but_keep_anchors_and_text() {
        assert_eq!(
            translate("=SUM( A1:$B2 ) + C$3 + $D$4 + 'A1 measurements'!E5", 2, 1),
            "=SUM( B3:$B4 ) + D$3 + $D$4 + 'A1 measurements'!F7"
        );
        assert_eq!(
            translate("=IF(A1=\"B2\",A2,3)", 1, 2),
            "=IF(C2=\"B2\",C3,3)"
        );
        assert_eq!(translate("A1 is a label", 2, 2), "A1 is a label");
        assert_eq!(translate("=A1+$B$2", 0, 0), "=A1+$B$2");
        assert_eq!(translate("=A1", -1, 0), "=#REF!");
        assert_eq!(translate("='Clay'!AZ1000", 1, 0), "=#REF!");
        assert_eq!(translate("=A9223372036854775808", 1, 0), "=#REF!");
        assert!(matches!(
            parse(&translate("=SUM(A1:B2)", -1, 0)),
            Ok(Expr::Call(_, _))
        ));
    }

    #[test]
    fn sheet_rename_and_delete_update_only_real_references() {
        let input = "=SUM('Clay tests'!A1:B2)+IF(TRUE,\"Clay tests!A1\",'Other'!A1)";
        let renamed = rename_sheet_references(input, "clay tests", "Artist's tests");
        assert_eq!(
            renamed,
            "=SUM('Artist''s tests'!A1:B2)+IF(TRUE,\"Clay tests!A1\",'Other'!A1)"
        );
        assert!(parse(&renamed).is_ok());
        let deleted = delete_sheet_references(input, "CLAY TESTS");
        assert_eq!(deleted, "=SUM(#REF!)+IF(TRUE,\"Clay tests!A1\",'Other'!A1)");
        assert!(parse(&deleted).is_ok());
        assert_eq!(delete_sheet_references("=A1:'Old'!B2", "Old"), "=#REF!");
        assert_eq!(
            delete_sheet_references("=Old!A1+Other!A1", "Old"),
            "=#REF!+Other!A1"
        );
    }

    #[test]
    fn malformed_and_deep_expressions_fail_with_bounded_errors() {
        for input in [
            "=",
            "=SUM(1,)",
            "=A1!",
            "=\"unterminated",
            "='bad!A1",
            "=1e+",
        ] {
            assert!(parse(input).is_err(), "accepted {input}");
        }
        assert_eq!(parse("=A0"), Ok(Expr::Error(FormulaError::Ref)));
        assert_eq!(parse("=A1001"), Ok(Expr::Error(FormulaError::Ref)));
        assert_eq!(parse("=BA1"), Ok(Expr::Error(FormulaError::Ref)));
        assert!(matches!(
            parse("=#REF!:A1"),
            Ok(Expr::Error(FormulaError::Ref))
        ));
        assert_eq!(
            parse(&format!("={}1{}", "(".repeat(200), ")".repeat(200))),
            Err(FormulaError::Limit)
        );
        assert_eq!(
            parse(&format!("=1{}", "+1".repeat(200))),
            Err(FormulaError::Limit)
        );
        assert_eq!(
            parse(&format!("={}", "1".repeat(4097))),
            Err(FormulaError::Limit)
        );
    }

    #[test]
    fn structural_insert_moves_absolute_references_and_preserves_sheet_scope() {
        let input = "= A1 + $B$2 + 'Clay tests'!C3 + Other!D4 + \"A1\"";
        assert_eq!(
            adjust_references(
                input,
                "Clay tests",
                "CLAY TESTS",
                Axis::Row,
                Edit::Insert { at: 1, count: 2 }
            ),
            "= A1 + $B$4 + 'Clay tests'!C5 + Other!D4 + \"A1\""
        );
        assert_eq!(
            adjust_references(
                "=A1+'Clay tests'!$A$1",
                "Other",
                "Clay tests",
                Axis::Column,
                Edit::Insert { at: 0, count: 1 }
            ),
            "=A1+'Clay tests'!$B$1"
        );
        assert_eq!(
            adjust_references(
                "=SUM('Clay tests'!A1:B3)+SUM(A1:B3)",
                "Other",
                "Clay tests",
                Axis::Column,
                Edit::Insert { at: 1, count: 1 }
            ),
            "=SUM('Clay tests'!A1:C3)+SUM(A1:B3)"
        );
    }

    #[test]
    fn structural_delete_shrinks_ranges_and_marks_deleted_cells() {
        let change = Edit::Delete { at: 2, count: 2 };
        assert_eq!(
            adjust_references(
                "=SUM(A2:A6)+A3 + $B$5",
                "Sheet1",
                "Sheet1",
                Axis::Row,
                change
            ),
            "=SUM(A2:A4)+#REF! + $B$3"
        );
        assert_eq!(
            adjust_references(
                "=SUM(A3:B4)+SUM(A6:A2)",
                "Sheet1",
                "Sheet1",
                Axis::Row,
                change
            ),
            "=SUM(#REF!)+SUM(A4:A2)"
        );
        assert_eq!(
            adjust_references(
                "=IF(FALSE,'Clay tests'!A3,1)",
                "Other",
                "Clay tests",
                Axis::Row,
                change
            ),
            "=IF(FALSE,#REF!,1)"
        );
        assert_eq!(
            adjust_references(
                "=SUM(A2:'Clay tests'!B6)",
                "Other",
                "Clay tests",
                Axis::Row,
                change
            ),
            "=SUM(A2:'Clay tests'!B4)"
        );
    }

    #[test]
    fn structural_range_boundaries_and_invalid_references_are_explicit() {
        assert_eq!(
            adjust_references(
                "=SUM(A3:A6)",
                "S",
                "S",
                Axis::Row,
                Edit::Delete { at: 0, count: 4 }
            ),
            "=SUM(A1:A2)"
        );
        assert_eq!(
            adjust_references(
                "=SUM(A2:A4)",
                "S",
                "S",
                Axis::Row,
                Edit::Insert { at: 4, count: 1 }
            ),
            "=SUM(A2:A4)"
        );
        assert_eq!(
            adjust_references(
                "=SUM(B1:D3)",
                "S",
                "S",
                Axis::Column,
                Edit::Delete { at: 2, count: 1 }
            ),
            "=SUM(B1:C3)"
        );
        assert_eq!(
            adjust_references(
                "=A1000+SUM(A999:A1000)",
                "S",
                "S",
                Axis::Row,
                Edit::Insert { at: 0, count: 1 }
            ),
            "=#REF!+SUM(#REF!)"
        );
        assert_eq!(
            adjust_references(
                "=A1001",
                "S",
                "S",
                Axis::Row,
                Edit::Delete { at: 0, count: 1 }
            ),
            "=#REF!"
        );
        assert_eq!(
            adjust_references(
                "=A1",
                "S",
                "S",
                Axis::Row,
                Edit::Delete {
                    at: usize::MAX,
                    count: usize::MAX
                }
            ),
            "=A1"
        );
        assert_eq!(
            adjust_references(
                "A1 is text",
                "S",
                "S",
                Axis::Row,
                Edit::Insert { at: 0, count: 1 }
            ),
            "A1 is text"
        );
    }

    #[test]
    fn deleted_ranges_match_the_surviving_original_cells() {
        // Compare range adjustment to independently deleting individual cells,
        // including reversed ranges and deletions across either endpoint.
        for first in 0..12 {
            for last in 0..12 {
                for at in 0..12 {
                    for count in 1..=4 {
                        let change = Edit::Delete { at, count };
                        let surviving: Vec<_> = (first.min(last)..=first.max(last))
                            .filter(|cell| *cell < at || *cell >= at + count)
                            .map(|cell| {
                                if cell >= at + count {
                                    cell - count
                                } else {
                                    cell
                                }
                            })
                            .collect();
                        let expected =
                            surviving.first().zip(surviving.last()).map(|(low, high)| {
                                if first <= last {
                                    (*low, *high)
                                } else {
                                    (*high, *low)
                                }
                            });
                        assert_eq!(change.range(first, last, MAX_ROWS), expected);
                    }
                }
            }
        }
    }
}
