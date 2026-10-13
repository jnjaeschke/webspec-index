//! Call construction (§8.5): the callee, its region, named arguments and the
//! execution hint.
use crate::state::grammar::{literal, Callability, Parser, Placeholder};
use crate::state::ir::{
    call_id, ArgName, Call, CallForm, Callee, ExecutionHint, Expr, LinkRole, NamedArg, NamedForm,
    SourceContext, StatementSource,
};
use crate::state::model::Literal;

/// Separators between `NAMEDARG`s, longest first.
const NAMED_SEPARATORS: [&str; 3] = [", and ", ", ", " and "];
/// Control words that start a clause of their own after a separator.
const CLAUSE_HEADS: [&str; 5] = ["return", "abort", "throw", "continue", "break"];
/// Words an unlinked `ARGNAME` ending in `flag` may span.
const MAX_FLAG_WORDS: usize = 6;

/// The key of `env.mentions` and `env.body_args`: the segment of an
/// algorithm source, else the source itself.
pub(crate) fn segment_key(source: &StatementSource) -> &str {
    match &source.context {
        SourceContext::Algorithm { segment_id, .. } => segment_id,
        _ => &source.id,
    }
}

/// A `NAMEDARG` head: its name and form, and where its value starts
/// (`None` for `FlagSet`, whose value is `true`), or where it ends.
struct NamedHead {
    name: ArgName,
    name_link: Option<usize>,
    form: NamedForm,
    value_start: Option<usize>,
    end: usize,
}

impl Parser<'_> {
    /// The call whose callee is link `link`, its region running from the
    /// link's end to `region_end` (encoded positions). `None` when the link
    /// is a mention or its target is not callable.
    pub(crate) fn call_at(
        &mut self,
        link: usize,
        form: CallForm,
        receiver: Option<Expr>,
        region_end: usize,
    ) -> Option<String> {
        let source = self.source;
        let callee_link = &source.links[link];
        let key = (segment_key(source).to_string(), callee_link.id.clone());
        if self.env.mentions.contains(&key)
            || self.env.callability(callee_link.target.as_ref()) == Callability::No
        {
            return None;
        }
        let id = call_id(&source.id, &callee_link.id);
        if self.out.calls.iter().any(|call| call.id == id) {
            return Some(id);
        }
        let (link_start, link_end) = self.link_range(link)?;
        let region_start = link_end.min(region_end);
        let region_end = self.clause_end(region_start, region_end);
        let (region_end, hint) = self.cut_in_parallel(region_start, region_end);
        let named = self.named_args(region_start, region_end, &id);
        self.set_role(link, LinkRole::Callee { call: id.clone() });
        self.out.calls.push(Call {
            id: id.clone(),
            source_id: source.id.clone(),
            statement_id: String::new(),
            parent_call: None,
            nested: Vec::new(),
            callee: Callee {
                link_id: callee_link.id.clone(),
                target: callee_link.target.clone(),
                visible_text: callee_link.visible_text.clone(),
            },
            form,
            span: self.enc.span(link_start, region_end),
            region: self.enc.span(region_start, region_end),
            receiver,
            named,
            body_args: self.env.body_args.get(&key).cloned().unwrap_or_default(),
            hint,
        });
        Some(id)
    }

    /// Encoded range of link `link`'s placeholder.
    fn link_range(&self, link: usize) -> Option<(usize, usize)> {
        let start = self.enc.to_enc(self.source.links[link].span.start);
        match self.enc.placeholder(start)? {
            (Placeholder::Link(index), end) if index == link => Some((start, end)),
            _ => None,
        }
    }

    /// `region_end`, or the first top-level separator before a clause of its
    /// own (`, and return`).
    fn clause_end(&self, region_start: usize, region_end: usize) -> usize {
        self.top_level(region_start, region_end)
            .into_iter()
            .find(|&at| {
                self.keyword(at, &NAMED_SEPARATORS).is_some_and(|after| {
                    CLAUSE_HEADS.iter().any(|word| {
                        self.lit(after, word) && !self.word_continues(after + word.len())
                    })
                })
            })
            .unwrap_or(region_end)
    }

    /// `region_end` without a trailing ` in parallel` (plain text or a link
    /// to `HTML#in-parallel`), and the hint it sets.
    fn cut_in_parallel(&self, region_start: usize, region_end: usize) -> (usize, ExecutionHint) {
        let before = &self.enc.text[region_start..region_end];
        if let Some(rest) = before.strip_suffix(" in parallel") {
            return (region_start + rest.len(), ExecutionHint::InParallel);
        }
        let in_parallel_link = self
            .enc
            .links_in(region_start, region_end)
            .filter(|&link| {
                self.source.links[link].target.as_ref().is_some_and(|t| {
                    t.spec.eq_ignore_ascii_case("HTML") && t.anchor == "in-parallel"
                })
            })
            .find_map(|link| {
                let (start, end) = self.link_range(link)?;
                (end == region_end).then_some(start)
            });
        match in_parallel_link {
            Some(start) if start > region_start && self.lit(start - 1, " ") => {
                (start - 1, ExecutionHint::InParallel)
            }
            _ => (region_end, ExecutionHint::Inline),
        }
    }

    /// `NAMED`: the named arguments at the end of `start..end`.
    fn named_args(&mut self, start: usize, end: usize, call: &str) -> Vec<NamedArg> {
        let Some(mut pos) = self.top_level(start, end).into_iter().find_map(|at| {
            let after = self.keyword(at, &[", with ", " with "])?;
            self.named_head(after, end).map(|_| after)
        }) else {
            return Vec::new();
        };
        let mut named = Vec::new();
        while let Some(head) = self.named_head(pos, end) {
            let (value, arg_end) = match head.value_start {
                Some(value_start) => {
                    let value_end = self.named_value_end(value_start, end);
                    (self.expr_at(value_start, value_end), value_end)
                }
                None => (Expr::Literal(Literal::Bool(true)), head.end),
            };
            if let Some(link) = head.name_link {
                self.set_role(
                    link,
                    LinkRole::ParamName {
                        call: call.to_string(),
                    },
                );
            }
            named.push(NamedArg {
                name: head.name,
                value,
                span: self.enc.span(pos, arg_end),
                form: head.form,
            });
            match self.keyword(arg_end, &NAMED_SEPARATORS) {
                Some(next) => pos = next,
                None => break,
            }
        }
        named
    }

    /// Positions in `start..end` outside parentheses, `« »`, links and code.
    pub(crate) fn top_level(&self, start: usize, end: usize) -> Vec<usize> {
        let mut positions = Vec::new();
        let mut depth = 0usize;
        let mut pos = start;
        while pos < end {
            if let Some((_, after)) = self.enc.placeholder(pos) {
                pos = after;
                continue;
            }
            let ch = self.enc.text[pos..].chars().next().expect("in bounds");
            if !self.enc.is_protected(pos) && !self.lit(pos, "`") {
                match ch {
                    '(' | '\u{AB}' => depth += 1,
                    ')' | '\u{BB}' => depth = depth.saturating_sub(1),
                    _ if depth == 0 => positions.push(pos),
                    _ => {}
                }
            }
            pos += ch.len_utf8();
        }
        positions
    }

    /// A value ends at the next top-level separator followed by a
    /// `NAMEDARG`, or at `end`.
    fn named_value_end(&self, start: usize, end: usize) -> usize {
        self.top_level(start, end)
            .into_iter()
            .find(|&at| {
                self.keyword(at, &NAMED_SEPARATORS)
                    .is_some_and(|next| self.named_head(next, end).is_some())
            })
            .unwrap_or(end)
    }

    /// A `NAMEDARG` head at `pos`: `ARGNAME set to`, `the ARGNAME set`, or
    /// `(its|the) ⟦C⟧ attribute(s)? initialized to`.
    fn named_head(&self, pos: usize, end: usize) -> Option<NamedHead> {
        if let Some(after) = self.keyword(pos, &["its ", "the "]) {
            if let Some((member, after_member)) = self.code_member(after) {
                let at = self.keyword(after_member, &[" attributes", " attribute"]);
                let at = at.unwrap_or(after_member);
                if let Some(value_start) = self.keyword(at, &[" initialized to "]) {
                    return (value_start < end).then(|| NamedHead {
                        name: ArgName::Text(member.clone()),
                        name_link: None,
                        form: NamedForm::AttributeInit { member },
                        value_start: Some(value_start),
                        end: value_start,
                    });
                }
            }
        }
        if let Some(after) = self.keyword(pos, &["the "]) {
            if let Some((name, name_link, after_name)) = self.arg_name(after) {
                if let Some(set_end) = self.keyword(after_name, &[" set"]) {
                    let closes = set_end == end
                        || (set_end < end && self.keyword(set_end, &NAMED_SEPARATORS).is_some());
                    if closes {
                        return Some(NamedHead {
                            name,
                            name_link,
                            form: NamedForm::FlagSet,
                            value_start: None,
                            end: set_end,
                        });
                    }
                }
            }
        }
        let (name, name_link, after_name) = self.arg_name(pos)?;
        if let Some(value_start) = self.keyword(after_name, &[" set to "]) {
            return (value_start < end).then_some(NamedHead {
                name,
                name_link,
                form: NamedForm::SetTo,
                value_start: Some(value_start),
                end: value_start,
            });
        }

        // Omitted `set to` is only unambiguous for linked parameter names and
        // a single value atom. Keep text and variable names on the old path.
        let link = name_link?;
        let value_start = self.keyword(after_name, &[" "])?;
        if value_start >= end {
            return None;
        }
        let value_end = self.named_value_end(value_start, end);
        let (trim_start, trim_end) = self.trimmed(value_start, value_end);
        let is_variable = matches!(self.enc.placeholder(trim_start), Some((Placeholder::Var(_), after)) if after == trim_end);
        let is_literal = literal(&self.enc.text[trim_start..trim_end])
            .is_some_and(|literal| !matches!(literal, Literal::Undefined));
        (is_variable || is_literal).then_some(NamedHead {
            name,
            name_link: Some(link),
            form: NamedForm::SetTo,
            value_start: Some(value_start),
            end: value_start,
        })
    }

    /// `ARGNAME`: a link, a variable, an identifier, or words ending in
    /// `flag`; with the link index of a linked name.
    fn arg_name(&self, pos: usize) -> Option<(ArgName, Option<usize>, usize)> {
        match self.enc.placeholder(pos) {
            Some((Placeholder::Link(link), after)) => {
                let link_span = &self.source.links[link];
                let name = ArgName::ParamLink {
                    link_id: link_span.id.clone(),
                    target: link_span.target.clone(),
                };
                return Some((name, Some(link), self.trailer(after)));
            }
            Some((Placeholder::Var(var), after)) => {
                let name = ArgName::Var(self.enc.vars[var].clone());
                return Some((name, None, self.trailer(after)));
            }
            None => {}
        }
        let bytes = self.enc.text.as_bytes();
        let word_end = |start: usize| {
            let mut at = start;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            at
        };
        if !bytes.get(pos)?.is_ascii_alphabetic() && bytes[pos] != b'_' {
            return None;
        }
        // Words ending in `flag`: hyphenated words, separated by spaces.
        let mut at = pos;
        for _ in 0..MAX_FLAG_WORDS {
            let mut end = word_end(at);
            while self.lit(end, "-") && bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric) {
                end = word_end(end + 1);
            }
            if end == at {
                break;
            }
            if &self.enc.text[at..end] == "flag" {
                return Some((
                    ArgName::Text(self.enc.text[pos..end].to_string()),
                    None,
                    end,
                ));
            }
            match self.keyword(end, &[" "]) {
                Some(next) => at = next,
                None => break,
            }
        }
        let end = word_end(pos);
        Some((
            ArgName::Text(self.enc.text[pos..end].to_string()),
            None,
            end,
        ))
    }

    /// A code member `` `name` `` or a link spelling one.
    fn code_member(&self, pos: usize) -> Option<(String, usize)> {
        if let Some((Placeholder::Link(link), after)) = self.enc.placeholder(pos) {
            let text = &self.source.links[link].visible_text;
            let member = text.strip_prefix('`')?.strip_suffix('`')?;
            return Some((member.to_string(), after));
        }
        self.code(pos)
    }
}

#[cfg(test)]
mod tests {
    use crate::state::grammar::{Callable, Encoded, Env, Parser};
    use crate::state::ir::{
        tests_support::sources, ArgName, CallForm, ExecutionHint, Expr, LinkRole, NamedForm,
    };
    use crate::state::model::Literal;
    use crate::state::testing::var;

    fn env(callables: &[(&str, Callable)]) -> Env {
        Env {
            spec: "HTML".into(),
            callables: callables.iter().map(|(a, c)| (a.to_string(), *c)).collect(),
            ..Env::default()
        }
    }

    fn call_of(
        step: &str,
        env: &Env,
    ) -> (
        crate::state::ir::StatementSource,
        Option<crate::state::ir::Call>,
        std::collections::BTreeMap<usize, LinkRole>,
    ) {
        let src = sources(&[step]).remove(0);
        let enc = Encoded::new(&src);
        let mut p = Parser::new(&enc, &src, env);
        let end = enc.text.trim_end_matches('.').len();
        let id = p.call_at(0, CallForm::Imperative, None, end);
        let call = id.and_then(|id| p.out.calls.iter().find(|c| c.id == id).cloned());
        (src.clone(), call, p.out.roles.clone())
    }

    #[test]
    fn linked_named_arguments_without_set_to() {
        let e = env(&[("open-window", Callable::Template)]);
        let (src, call, _) = call_of(
            "<a href=\"#open-window\">Open</a> <var>url</var> with <a href=\"#exceptions-enabled\">exceptionsEnabled</a> true, and <a href=\"#navigation-hh\">historyHandling</a> \"replace\".",
            &e,
        );
        let call = call.unwrap();
        assert_eq!(call.named.len(), 2);
        assert!(
            matches!(&call.named[0].name, ArgName::ParamLink { target: Some(t), .. } if t.anchor == "exceptions-enabled")
        );
        assert_eq!(call.named[0].value, Expr::Literal(Literal::Bool(true)));
        assert!(
            matches!(&call.named[1].name, ArgName::ParamLink { target: Some(t), .. } if t.anchor == "navigation-hh")
        );
        assert!(
            matches!(&call.named[1].value, Expr::Literal(Literal::String(value)) if value == "replace")
        );
        assert!(src.text[call.region.start..call.region.end].contains("*url*"));
    }

    #[test]
    fn linked_named_argument_without_set_to_after_using() {
        let e = env(&[("navigate", Callable::Template)]);
        let (src, call, _) = call_of(
            "<a href=\"#navigate\">Navigate</a> <var>navigable</var> to <var>url</var> using <var>browsingContext</var>'s associated document, with <a href=\"#exceptions-enabled\">exceptionsEnabled</a> true.",
            &e,
        );
        let call = call.unwrap();
        assert_eq!(call.named.len(), 1);
        assert!(
            matches!(&call.named[0].name, ArgName::ParamLink { target: Some(t), .. } if t.anchor == "exceptions-enabled")
        );
        assert_eq!(call.named[0].value, Expr::Literal(Literal::Bool(true)));
        assert!(src.text[call.region.start..call.named[0].span.start]
            .contains("*browsingContext*'s associated document"));
        assert_eq!(call.region.end, call.named[0].span.end);
    }

    #[test]
    fn unlinked_name_without_set_to_is_not_guessed() {
        let e = env(&[("fetch", Callable::Template)]);
        let (src, call, _) = call_of(
            "<a href=\"#fetch\">Fetch</a> <var>request</var> with <var>x</var> true.",
            &e,
        );
        let call = call.unwrap();
        assert!(call.named.is_empty());
        assert!(src.text[call.region.start..call.region.end].contains("*x* true"));
    }

    #[test]
    fn g2_named_arguments_by_parameter_link() {
        let e = env(&[("navigate", Callable::Template)]);
        let (src, call, roles) = call_of("<a href=\"#navigate\">Navigate</a> <var>navigable</var> to <var>url</var> using <var>sourceDocument</var>, with <a href=\"#exceptions-enabled\"><var>exceptionsEnabled</var></a> set to true and <a href=\"#navigation-hh\"><var>historyHandling</var></a> set to <var>historyHandling</var>.", &e);
        let call = call.unwrap();
        assert_eq!(call.callee.target.as_ref().unwrap().anchor, "navigate");
        assert_eq!(call.named.len(), 2);
        assert!(
            matches!(&call.named[0].name, ArgName::ParamLink { target: Some(t), .. } if t.anchor == "exceptions-enabled")
        );
        assert_eq!(call.named[0].value, Expr::Literal(Literal::Bool(true)));
        assert_eq!(call.named[0].form, NamedForm::SetTo);
        assert_eq!(call.named[1].value, var("historyHandling"));
        assert_eq!(call.hint, ExecutionHint::Inline);
        assert_eq!(&src.text[call.span.start..call.span.start + 8], "Navigate");
        assert!(src.text[call.region.start..call.region.end]
            .contains("*navigable* to *url* using *sourceDocument*"));
        assert_eq!(
            roles[&0],
            LinkRole::Callee {
                call: call.id.clone()
            }
        );
        assert_eq!(
            roles[&1],
            LinkRole::ParamName {
                call: call.id.clone()
            }
        );
    }

    #[test]
    fn flag_set_attribute_init_and_in_parallel() {
        let e = env(&[("fetch", Callable::Template)]);
        let (_, call, _) = call_of("<a href=\"#fetch\">Fetch</a> <var>request</var> with the <a href=\"#same-origin-fallback\">same-origin fallback flag</a> set, and its <code>bubbles</code> attribute initialized to true.", &e);
        let call = call.unwrap();
        assert_eq!(call.named[0].form, NamedForm::FlagSet);
        assert_eq!(call.named[0].value, Expr::Literal(Literal::Bool(true)));
        assert_eq!(
            call.named[1].form,
            NamedForm::AttributeInit {
                member: "bubbles".into()
            }
        );
        assert_eq!(call.named[1].name, ArgName::Text("bubbles".into()));
        let (src, call, _) = call_of(
            "<a href=\"#fetch\">Fetch</a> <var>request</var> <a href=\"#in-parallel\">in parallel</a>.",
            &e,
        );
        let call = call.unwrap();
        assert_eq!(call.hint, ExecutionHint::InParallel);
        assert_eq!(
            src.text[call.region.start..call.region.end].trim(),
            "*request*"
        );
    }

    #[test]
    fn unlinked_names_nesting_plain_in_parallel_and_body_args() {
        let e = env(&[("fetch", Callable::Template)]);
        let (_, call, _) = call_of("<a href=\"#fetch\">Fetch</a> <var>request</var> with the same-origin fallback flag set and <var>mode</var> set to \"<code>cors</code>\", and useParallelQueue set to true.", &e);
        let call = call.unwrap();
        assert_eq!(call.named.len(), 3);
        assert_eq!(
            call.named[0].name,
            ArgName::Text("same-origin fallback flag".into())
        );
        assert_eq!(call.named[0].form, NamedForm::FlagSet);
        assert_eq!(call.named[1].name, ArgName::Var("mode".into()));
        assert!(matches!(&call.named[1].value, Expr::EnumValue { text, .. } if text == "cors"));
        assert_eq!(call.named[2].name, ArgName::Text("useParallelQueue".into()));

        let (src, call, _) = call_of("<a href=\"#fetch\">Fetch</a> (<var>a</var> with <var>b</var> set to 1) with nothing in particular in parallel.", &e);
        let call = call.unwrap();
        assert!(call.named.is_empty());
        assert_eq!(call.hint, ExecutionHint::InParallel);
        assert!(src.text[call.span.start..call.span.end].ends_with("in particular"));

        let src = sources(&[
            "<a href=\"#fetch\">Fetch</a> <var>request</var>, with <var>x</var> set to 1.",
        ])
        .remove(0);
        let mut e = env(&[("fetch", Callable::Template)]);
        let key = (
            super::segment_key(&src).to_string(),
            src.links[0].id.clone(),
        );
        e.body_args.insert(key, vec!["body-1".into()]);
        let enc = Encoded::new(&src);
        let mut p = Parser::new(&enc, &src, &e);
        let end = enc.text.trim_end_matches('.').len();
        let first = p.call_at(0, CallForm::Imperative, None, end).unwrap();
        assert_eq!(p.call_at(0, CallForm::Imperative, None, end), Some(first));
        assert_eq!(p.out.calls.len(), 1);
        let call = &p.out.calls[0];
        assert_eq!(call.body_args, vec!["body-1".to_string()]);
        assert_eq!(call.statement_id, "");
        assert!(call.parent_call.is_none() && call.nested.is_empty());
        assert_eq!(
            &src.text[call.named[0].span.start..call.named[0].span.end],
            "*x* set to 1"
        );
    }

    #[test]
    fn mentions_and_uncallable_links_are_never_callees() {
        let src = sources(&["<a href=\"#fetch\">Fetch</a> <var>request</var>."]).remove(0);
        let mut e = env(&[("fetch", Callable::Template)]);
        let crate::state::ir::SourceContext::Algorithm { segment_id, .. } = &src.context else {
            panic!()
        };
        e.mentions
            .insert((segment_id.clone(), src.links[0].id.clone()));
        let enc = Encoded::new(&src);
        let mut p = Parser::new(&enc, &src, &e);
        assert_eq!(
            p.call_at(0, CallForm::Imperative, None, enc.text.len()),
            None
        );
        let (_, call, roles) = call_of(
            "<a href=\"#navigable\">Navigable</a> <var>x</var>.",
            &env(&[]),
        );
        assert!(call.is_none() && roles.is_empty());
    }
}
