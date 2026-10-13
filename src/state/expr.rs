//! `VALUE` grammar: expression atoms (§7.3), extended with calls and
//! conditionals by later productions.
use crate::state::call::segment_key;
use crate::state::grammar::{literal, Callability, Callable, Parser, Placeholder};
use crate::state::ir::{CallForm, Expr, LinkRole, Path, Root};
use crate::state::model::Literal;

const QUOTES: [(&str, &str); 2] = [("\"", "\""), ("\u{201C}", "\u{201D}")];

/// A possessive call's receiver: its path without the accessor hop. A bare
/// variable or `this` is that expression; a receiver-less path has none.
fn path_receiver(path: Path) -> Option<Expr> {
    if !path.hops.is_empty() || path.subscript.is_some() {
        return Some(Expr::Path(path));
    }
    match path.root {
        Root::Var(name) => Some(Expr::Var(name)),
        Root::This => Some(Expr::This),
        Root::Implicit => None,
        Root::Opaque { text } => Some(Expr::Opaque { text }),
        root @ Root::Link { .. } => Some(Expr::Path(Path {
            root,
            hops: Vec::new(),
            subscript: None,
        })),
    }
}

impl Parser<'_> {
    /// `VALUE` → `Expr`. Every link inside a parsed (non-`Opaque`) atom gets
    /// the `Value` role.
    /// `COND` is tried first. Calls (§8.5) are tried after `NEW` and before
    /// `PATH`; their links get roles from `call_at`, an algorithm value's link
    /// `AlgorithmValue`, and the type link of `a new ⟦T⟧` `Type`.
    pub(crate) fn expr_at(&mut self, start: usize, end: usize) -> Expr {
        let (start, end) = self.trimmed(start, end);
        if let Some(expr) = self.conditional(start, end) {
            return expr;
        }
        let mut values = Vec::new();
        let expr = match self.head_atom(start, end, &mut values) {
            Some(expr) => {
                if matches!(expr, Expr::New { .. }) {
                    if let Some(link) = self.new_type_link(start) {
                        self.set_role(link, LinkRole::Type);
                    }
                }
                self.parsed_values(expr, start, end, &mut values)
            }
            None => match self.call_expr(start, end) {
                Some(expr) => expr,
                None => {
                    let expr = self.tail_atom(start, end);
                    self.parsed_values(expr, start, end, &mut values)
                }
            },
        };
        for link in values {
            self.set_role(link, LinkRole::Value);
        }
        expr
    }

    /// `COND`: `A(,)? if PRED; otherwise(,)? B` at top level.
    fn conditional(&mut self, start: usize, end: usize) -> Option<Expr> {
        let text = &self.enc.text[start..end];
        if !text.contains(" if ") || !text.contains("; otherwise") {
            return None;
        }
        let positions = self.top_level(start, end);
        let if_at = *positions
            .iter()
            .find(|&&pos| pos > start && self.lit(pos, " if "))?;
        let otherwise_at = *positions
            .iter()
            .find(|&&pos| pos > if_at && self.lit(pos, "; otherwise"))?;
        let then_end = if self.enc.text[..if_at].ends_with(',') {
            if_at - 1
        } else {
            if_at
        };
        let after = otherwise_at + "; otherwise".len();
        let otherwise_start = self.keyword(after, &[","]).unwrap_or(after);
        let condition = self.predicate_at(if_at + " if ".len(), otherwise_at);
        let then = self.expr_at(start, then_end);
        let otherwise = self.expr_at(otherwise_start, end);
        Some(Expr::Conditional {
            condition: Box::new(condition),
            then: Box::new(then),
            otherwise: Box::new(otherwise),
        })
    }

    /// `VALUE` → `Expr` without calls and without recording roles; the link
    /// indices of parsed atoms go to `values`. `PATH` subscripts use it
    /// directly.
    pub(crate) fn value_expr(&self, start: usize, end: usize, values: &mut Vec<usize>) -> Expr {
        let (start, end) = self.trimmed(start, end);
        let expr = self
            .head_atom(start, end, values)
            .unwrap_or_else(|| self.tail_atom(start, end));
        self.parsed_values(expr, start, end, values)
    }

    pub(crate) fn trimmed(&self, start: usize, end: usize) -> (usize, usize) {
        let text = &self.enc.text[start..end];
        let start = start + (text.len() - text.trim_start().len());
        let end = end - (text.len() - text.trim_end().len());
        (start.min(end), end)
    }

    /// Adds the links of a parsed atom to `values`.
    fn parsed_values(&self, expr: Expr, start: usize, end: usize, values: &mut Vec<usize>) -> Expr {
        if !matches!(expr, Expr::Opaque { .. } | Expr::List(_)) {
            values.extend(self.enc.links_in(start, end));
        }
        expr
    }

    /// The type link of `a new ⟦T⟧` at `start`.
    fn new_type_link(&self, start: usize) -> Option<usize> {
        let after = self.keyword(start, &["a new ", "an new "])?;
        match self.enc.placeholder(after)? {
            (Placeholder::Link(link), _) => Some(link),
            _ => None,
        }
    }

    /// The atoms tried before calls: variables, `this`, enum values in
    /// quotes, literals, lists and `NEW`.
    fn head_atom(&self, start: usize, end: usize, values: &mut Vec<usize>) -> Option<Expr> {
        let text = &self.enc.text[start..end];
        match self.enc.placeholder(start) {
            Some((Placeholder::Var(var), after)) if after == end => {
                return Some(Expr::Var(self.enc.vars[var].clone()));
            }
            Some((Placeholder::Link(link), after)) if after == end && self.is_this_link(link) => {
                return Some(Expr::This);
            }
            _ => {}
        }
        if text == "this" {
            return Some(Expr::This);
        }
        // A quoted code token is an enum value, not the string `literal`
        // reads it as.
        if QUOTES
            .iter()
            .any(|(open, close)| text.starts_with(open) && text.ends_with(close))
        {
            if let Some(value) = self.enum_value(start, end) {
                return Some(value);
            }
        }
        if let Some(lit) = self.literal_at(start, end) {
            return Some(Expr::Literal(lit));
        }
        if let Some(items) = self.list(start, end) {
            return Some(Expr::List(
                items
                    .into_iter()
                    .map(|(start, end)| self.value_expr(start, end, values))
                    .collect(),
            ));
        }
        if let Some(after) = self.keyword(start, &["a new ", "an new "]) {
            return Some(Expr::New {
                ty: self.new_type(after),
                init: self.inits.get(&start).cloned(),
            });
        }
        None
    }

    /// The atoms tried after calls: `PATH`, enum values, else `Opaque`.
    fn tail_atom(&self, start: usize, end: usize) -> Expr {
        if let Some(path) = self.path(start, false).filter(|path| path.end == end) {
            return Expr::Path(path.path);
        }
        if let Some(value) = self.enum_value(start, end) {
            return value;
        }
        Expr::Opaque {
            text: self.src_text(start, end),
        }
    }

    /// A call spanning `start..end` (§8.5 value forms), or an algorithm named
    /// as a value.
    fn call_expr(&mut self, start: usize, end: usize) -> Option<Expr> {
        if start >= end {
            return None;
        }
        // RESULTOF
        if let Some((link, _)) = self
            .result_of_link(start)
            .filter(|&(_, after)| after <= end)
        {
            if self.link_callability(link) != Callability::No {
                return self.call_expr_at(link, CallForm::ResultOf, None, end);
            }
        }
        if let Some((link, close)) = self.ecmarkup_call(start, end) {
            if self.link_callability(link) != Callability::No {
                return self.call_expr_at(link, CallForm::Ecmarkup, None, close + 1);
            }
        }
        if let Some(link) = self.gerund_link(start, end) {
            if self.link_callability(link) != Callability::No {
                return self.call_expr_at(link, CallForm::Gerund, None, end);
            }
        }
        if let Some((Placeholder::Link(link), after)) = self.enc.placeholder(start) {
            let callability = self.link_callability(link);
            // CALLX
            let callx = match callability {
                Callability::Known(Callable::Body | Callable::Template | Callable::NoTemplate) => {
                    true
                }
                Callability::CrossSpec => self.keyword(after, &[" given ", " with "]).is_some(),
                _ => false,
            };
            if after < end && callx {
                return self.call_expr_at(link, CallForm::ResultOf, None, end);
            }
            // ALGREF
            if after == end && matches!(callability, Callability::Known(_)) {
                self.set_role(link, LinkRole::AlgorithmValue);
                let link = &self.source.links[link];
                return Some(Expr::AlgorithmRef {
                    link_id: link.id.clone(),
                    target: link.target.clone(),
                });
            }
        }
        self.possessive_call(start, end)
    }

    /// `PATH` whose last hop is an accessor, or `the ⟦L⟧ of R` with an
    /// accessor `⟦L⟧`: a `Possessive` call with the rest as its receiver.
    fn possessive_call(&mut self, start: usize, end: usize) -> Option<Expr> {
        let accessor = |p: &Self, link: usize| {
            p.link_callability(link) == Callability::Known(Callable::Accessor)
        };
        if let Some(path) = self
            .path(start, false)
            .filter(|path| path.end == end && path.path.subscript.is_none())
        {
            if let Some(&Some(link)) = path.hop_links.last() {
                if accessor(self, link) {
                    let mut receiver = path.path;
                    receiver.hops.pop();
                    let link_start = self.enc.to_enc(self.source.links[link].span.start);
                    let receiver_links: Vec<usize> = self.enc.links_in(start, link_start).collect();
                    for receiver_link in receiver_links {
                        self.set_role(receiver_link, LinkRole::Value);
                    }
                    return self.call_expr_at(
                        link,
                        CallForm::Possessive,
                        path_receiver(receiver),
                        end,
                    );
                }
            }
        }
        let at = self.keyword(start, &["the "])?;
        let (Placeholder::Link(link), after) = self.enc.placeholder(at)? else {
            return None;
        };
        let receiver_start = self.keyword(after, &[" of "]).filter(|&r| r < end)?;
        if !accessor(self, link) {
            return None;
        }
        let receiver = self.expr_at(receiver_start, end);
        self.call_expr_at(link, CallForm::Possessive, Some(receiver), end)
    }

    fn call_expr_at(
        &mut self,
        link: usize,
        form: CallForm,
        receiver: Option<Expr>,
        region_end: usize,
    ) -> Option<Expr> {
        self.call_at(link, form, receiver, region_end)
            .map(Expr::Call)
    }

    /// Callability of link `link` at this site: `No` for a mention.
    pub(crate) fn link_callability(&self, link: usize) -> Callability {
        let link = &self.source.links[link];
        let key = (segment_key(self.source).to_string(), link.id.clone());
        if self.env.mentions.contains(&key) {
            return Callability::No;
        }
        self.env.callability(link.target.as_ref())
    }

    /// `the result of ` (`running `|`performing `|`invoking `|`calling `)?
    /// (`the `)? `⟦L⟧` at `pos`: the link and the position after it.
    pub(crate) fn result_of_link(&self, pos: usize) -> Option<(usize, usize)> {
        let at = self.keyword(pos, &["the result of "])?;
        let at = self
            .keyword(at, &["running ", "performing ", "invoking ", "calling "])
            .unwrap_or(at);
        let at = self.keyword(at, &["the "]).unwrap_or(at);
        match self.enc.placeholder(at)? {
            (Placeholder::Link(link), after) => Some((link, after)),
            _ => None,
        }
    }

    /// `(? |! )?⟦L⟧(` … `)` spanning `start..end`: the link and the position
    /// of the closing parenthesis.
    fn ecmarkup_call(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let at = self.keyword(start, &["? ", "! "]).unwrap_or(start);
        let (Placeholder::Link(link), open) = self.enc.placeholder(at)? else {
            return None;
        };
        if !self.lit(open, "(") {
            return None;
        }
        let close = self.matching_paren(open, end)?;
        (close + 1 == end).then_some((link, close))
    }

    /// The `)` matching the `(` at `open`, outside protected ranges.
    fn matching_paren(&self, open: usize, end: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (offset, ch) in self.enc.text[open..end].char_indices() {
            let pos = open + offset;
            if self.enc.is_protected(pos) {
                continue;
            }
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// `GERUND`: a link at `start` whose text's first word ends in `ing`, or
    /// a word ending in `ing` directly before a link.
    fn gerund_link(&self, start: usize, end: usize) -> Option<usize> {
        if let Some((Placeholder::Link(link), _)) = self.enc.placeholder(start) {
            let text = &self.source.links[link].visible_text;
            let first = text.split_whitespace().next().unwrap_or_default();
            return first.ends_with("ing").then_some(link);
        }
        let word_len = self.enc.text[start..end]
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count();
        let word = &self.enc.text[start..start + word_len];
        if word.len() <= "ing".len() || !word.ends_with("ing") {
            return None;
        }
        let at = self.keyword(start + word_len, &[" "])?;
        match self.enc.placeholder(at)? {
            (Placeholder::Link(link), _) => Some(link),
            _ => None,
        }
    }

    /// `LIT`: `literal`, `failure`, `the empty string`, a number with a
    /// U+2212 minus, or a code token spelling a literal.
    pub(crate) fn literal_at(&self, start: usize, end: usize) -> Option<Literal> {
        let text = &self.enc.text[start..end];
        match text {
            "failure" => return Some(Literal::Failure),
            "the empty string" => return Some(Literal::String(String::new())),
            _ => {}
        }
        if let Some(digits) = text.strip_prefix('\u{2212}') {
            if let Some(Literal::Number(n)) = literal(digits).filter(|_| !digits.starts_with('-')) {
                return Some(Literal::Number(format!("-{n}")));
            }
        }
        if let Some((inner, after)) = self.code(start) {
            if after == end {
                return literal(&inner);
            }
        }
        // Placeholder text is never a literal's content.
        if text.contains('\u{27E6}') {
            return None;
        }
        literal(text)
    }

    /// `LIST`: `« »` or `« A, B »` spanning `start..end`; the element ranges.
    fn list(&self, start: usize, end: usize) -> Option<Vec<(usize, usize)>> {
        let inner_start = self.keyword(start, &["\u{AB}"])?;
        let inner_end = self.enc.text[..end].strip_suffix('\u{BB}')?.len();
        if inner_end < inner_start {
            return None;
        }
        let mut items = Vec::new();
        let mut item_start = inner_start;
        let mut depth = 0usize;
        let mut pos = inner_start;
        while pos < inner_end {
            if let Some((_, after)) = self.enc.placeholder(pos) {
                pos = after;
                continue;
            }
            let ch = self.enc.text[pos..].chars().next().expect("in bounds");
            if !self.enc.is_protected(pos) {
                match ch {
                    '\u{AB}' | '(' => depth += 1,
                    // The opening `«` closes before the end: not one list.
                    '\u{BB}' | ')' => depth = depth.checked_sub(1)?,
                    ',' if depth == 0 && self.lit(pos, ", ") => {
                        items.push((item_start, pos));
                        item_start = pos + ", ".len();
                    }
                    _ => {}
                }
            }
            pos += ch.len_utf8();
        }
        if depth != 0 {
            return None;
        }
        if !self.enc.text[item_start..inner_end].trim().is_empty() || !items.is_empty() {
            items.push((item_start, inner_end));
        }
        Some(items)
    }

    /// `ENUM`: a code token or a link with backtick-delimited text, either
    /// optionally quoted.
    fn enum_value(&self, start: usize, end: usize) -> Option<Expr> {
        let text = &self.enc.text[start..end];
        let (start, end) = QUOTES
            .iter()
            .find(|(open, close)| {
                text.len() >= open.len() + close.len()
                    && text.starts_with(open)
                    && text.ends_with(close)
            })
            .map_or((start, end), |(open, close)| {
                (start + open.len(), end - close.len())
            });
        if let Some((Placeholder::Link(link), after)) = self.enc.placeholder(start) {
            let link = &self.source.links[link];
            let text = link.visible_text.strip_prefix('`')?.strip_suffix('`')?;
            return (after == end).then(|| Expr::EnumValue {
                text: text.to_string(),
                target: link.target.clone(),
            });
        }
        let (inner, after) = self.code(start)?;
        (after == end && self.enc.is_protected(start + 1)).then_some(Expr::EnumValue {
            text: inner,
            target: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::state::grammar::{Encoded, Env, Parser};
    use crate::state::ir::{tests_support::sources, Expr, Hop, Path, Root};
    use crate::state::model::Literal;
    use crate::state::testing::var;

    /// Parse the whole of `html` (one step) as an expression.
    fn expr(html: &str) -> Expr {
        let src = sources(&[html]).remove(0);
        let enc = Encoded::new(&src);
        let env = Env::default();
        let mut p = Parser::new(&enc, &src, &env);
        let end = enc.text.trim_end_matches('.').len();
        p.expr_at(0, end)
    }

    #[test]
    fn atoms() {
        assert_eq!(expr("<var>x</var>"), var("x"));
        assert_eq!(
            expr("<a href=\"https://webidl.spec.whatwg.org/#this\">this</a>"),
            Expr::This
        );
        assert_eq!(
            expr("the empty string"),
            Expr::Literal(Literal::String(String::new()))
        );
        assert_eq!(expr("failure"), Expr::Literal(Literal::Failure));
        assert_eq!(expr("−1"), Expr::Literal(Literal::Number("-1".into())));
        assert_eq!(
            expr("<code>true</code>"),
            Expr::Literal(Literal::Bool(true))
        );
    }

    #[test]
    fn lists_and_enum_values() {
        assert_eq!(expr("« »"), Expr::List(vec![]));
        assert_eq!(expr("« <var>node</var> »"), Expr::List(vec![var("node")]));
        assert_eq!(
            expr("« <var>a</var>, « <var>b</var>, <var>c</var> » »"),
            Expr::List(vec![var("a"), Expr::List(vec![var("b"), var("c")])])
        );
        // An unclosed `«` ending in a placeholder is no list (URLPATTERN, WEBDRIVER).
        assert!(!matches!(expr("« <var>a</var>"), Expr::List(_)));
        assert_eq!(
            expr("\"<code>replace</code>\""),
            Expr::EnumValue {
                text: "replace".into(),
                target: None
            }
        );
        let Expr::EnumValue { text, target } =
            expr("\"<code><a href=\"#nhb-auto\">auto</a></code>\"")
        else {
            panic!()
        };
        assert_eq!(
            (text.as_str(), target.unwrap().anchor.as_str()),
            ("auto", "nhb-auto")
        );
    }

    #[test]
    fn paths_and_opaque() {
        let Expr::Path(Path {
            root: Root::Var(root),
            hops,
            ..
        }) = expr("<var>document</var>'s <a href=\"#url\">URL</a>")
        else {
            panic!()
        };
        assert_eq!(root, "document");
        assert!(matches!(&hops[0], Hop::Field { visible_text, .. } if visible_text == "URL"));
        assert!(matches!(
            expr("the number of seconds since midnight"),
            Expr::Opaque { .. }
        ));
    }

    #[test]
    fn value_role_only_for_parsed_expressions() {
        let src = sources(&["<var>d</var>'s <a href=\"#url\">URL</a>"]).remove(0);
        let enc = Encoded::new(&src);
        let env = Env::default();
        let mut p = Parser::new(&enc, &src, &env);
        let _ = p.expr_at(0, enc.text.len());
        assert_eq!(
            p.out.roles.get(&0),
            Some(&crate::state::ir::LinkRole::Value)
        );

        let src = sources(&["the number of <a href=\"#url\">URLs</a> seen"]).remove(0);
        let enc = Encoded::new(&src);
        let mut p = Parser::new(&enc, &src, &env);
        assert!(matches!(p.expr_at(0, enc.text.len()), Expr::Opaque { .. }));
        assert!(p.out.roles.is_empty());
    }

    #[test]
    fn a_list_must_span_the_whole_range() {
        assert!(matches!(
            expr("« <var>a</var> » and « <var>b</var> »"),
            Expr::Opaque { .. }
        ));
    }
}
