//! Source model scripts (`.MDS`).
//!
//! The syntax looks like function calls, `ani("S_RUN" 1 "S_RUN" 0.1 0.1 M. "HUM_RUN.ASC" F 1 20)`,
//! but the engine ignores parentheses entirely: a script is just a stream of keywords, strings and
//! numbers, with braces for blocks. Handwritten scripts, mods' especially, are full of missing
//! quotes, parentheses and braces that the engine shrugs off, so this parser follows ZenKit's in
//! accepting them.

use tracing::warn;

use super::Animation;
use super::AnimationAlias;
use super::AnimationBlend;
use super::AnimationCombination;
use super::AnimationFlags;
use super::CameraTremor;
use super::Direction;
use super::EventTag;
use super::FORMAT;
use super::ModelScript;
use super::MorphAnimation;
use super::ParticleEffect;
use super::ParticleEffectStop;
use super::Skeleton;
use super::SoundEffect;
use crate::Error;
use crate::Result;
use crate::io::WHITESPACE;
use crate::text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyword,
    Int,
    Float,
    String,
    LBrace,
    RBrace,
    Colon,
    Eof,
    Illegal,
}

impl Kind {
    fn describe(self) -> &'static str {
        match self {
            Kind::Keyword => "a keyword",
            Kind::Int => "an integer",
            Kind::Float => "a number",
            Kind::String => "a string",
            Kind::LBrace => "`{`",
            Kind::RBrace => "`}`",
            Kind::Colon => "`:`",
            Kind::Eof => "the end of the file",
            Kind::Illegal => "an illegal character",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Token<'a> {
    kind: Kind,
    text: &'a [u8],
    start: usize,
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    /// The last token read, with the positions before and after it. The parser often reads a
    /// token only to step back, and this saves lexing it again, and warning about it twice.
    last: Option<(usize, Token<'a>, usize)>,
}

impl<'a> Lexer<'a> {
    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn eat_while(&mut self, f: impl Fn(u8) -> bool) {
        while self.peek_byte().is_some_and(&f) {
            self.pos += 1;
        }
    }

    fn next(&mut self) -> Token<'a> {
        if let Some((before, token, after)) = self.last
            && before == self.pos
        {
            self.pos = after;
            return token;
        }

        let before = self.pos;
        let token = self.lex();
        self.last = Some((before, token, self.pos));
        token
    }

    fn lex(&mut self) -> Token<'a> {
        loop {
            let start = self.pos;
            let token =
                |lexer: &Self, kind| Token { kind, text: &lexer.src[start..lexer.pos], start };
            let Some(c) = self.peek_byte() else {
                return token(self, Kind::Eof);
            };
            self.pos += 1;

            match c {
                c if WHITESPACE.contains(&c) || matches!(c, b'(' | b')' | b';') => {}
                b'/' => {
                    if self.peek_byte() != Some(b'/') {
                        warn!("{}: comment with a single slash", self.at(start));
                    }
                    self.eat_while(|c| c != b'\n');
                }
                c if c.is_ascii_alphabetic() || matches!(c, b'*' | b'_' | b'.') => {
                    self.eat_while(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')
                    });
                    return token(self, Kind::Keyword);
                }
                c if c.is_ascii_digit() || c == b'-' => {
                    self.eat_while(|c| c.is_ascii_digit() || c == b'.');
                    let t = token(self, Kind::Int);
                    let kind = if t.text.contains(&b'.') { Kind::Float } else { Kind::Int };
                    return Token { kind, ..t };
                }
                b'"' => return self.string(start),
                b'{' => return token(self, Kind::LBrace),
                b'}' => return token(self, Kind::RBrace),
                b':' => return token(self, Kind::Colon),
                _ => {
                    warn!("{}: illegal character {:?}", self.at(start), c as char);
                    return token(self, Kind::Illegal);
                }
            }
        }
    }

    /// A string ends at its closing quote or, if that is missing, at the end of the line. Then a
    /// trailing `)` is taken to be the call's, as in `registerMesh("BODY.ASC)`. ZenKit drops a
    /// trailing `)` even inside closed quotes; here `"BODY(1)"` keeps it.
    fn string(&mut self, start: usize) -> Token<'a> {
        let body = &self.src[start + 1..];
        let len =
            body.iter().position(|&c| matches!(c, b'"' | b'\n' | b'\r')).unwrap_or(body.len());

        let mut text = &body[..len];
        if body.get(len) == Some(&b'"') {
            self.pos = start + 1 + len + 1;
        } else {
            warn!("{}: string is not closed", self.at(start));
            self.pos = start + 1 + len;
            text = text.trim_ascii_end();
            text = text.strip_suffix(b")").unwrap_or(text);
        }

        Token { kind: Kind::String, text, start }
    }

    /// `line L, column C` of a byte offset, for messages.
    fn at(&self, offset: usize) -> String {
        let before = &self.src[..offset];
        let line = before.iter().filter(|&&c| c == b'\n').count() + 1;
        let column = offset - before.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1) + 1;
        format!("line {line}, column {column}")
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
}

fn string(t: Token) -> String {
    text::latin1(t.text)
}

fn is(t: Token, keyword: &str) -> bool {
    t.text.eq_ignore_ascii_case(keyword.as_bytes())
}

/// Whether a keyword is made up of animation flag letters and placeholder dots only.
fn is_flags(t: Token) -> bool {
    t.text.iter().all(|c| b"MREFIP.".contains(&c.to_ascii_uppercase()))
}

impl<'a> Parser<'a> {
    fn error(&self, t: Token, message: impl std::fmt::Display) -> Error {
        Error::malformed(FORMAT, format!("{}: {message}", self.lexer.at(t.start)))
    }

    fn maybe(&mut self, kind: Kind) -> Option<Token<'a>> {
        let saved = self.lexer.pos;
        let t = self.lexer.next();
        if t.kind == kind {
            Some(t)
        } else {
            self.lexer.pos = saved;
            None
        }
    }

    fn expect(&mut self, kind: Kind) -> Result<Token<'a>> {
        let t = self.lexer.next();
        if t.kind == kind {
            Ok(t)
        } else {
            Err(self.error(t, format!("expected {}, found {}", kind.describe(), t.kind.describe())))
        }
    }

    fn maybe_keyword(&mut self, keyword: &str) -> bool {
        let saved = self.lexer.pos;
        match self.maybe(Kind::Keyword) {
            Some(t) if is(t, keyword) => true,
            _ => {
                self.lexer.pos = saved;
                false
            }
        }
    }

    fn string(&mut self) -> Result<String> {
        self.expect(Kind::String).map(string)
    }

    fn maybe_string(&mut self) -> Option<String> {
        self.maybe(Kind::String).map(string)
    }

    fn parse_int(&self, t: Token) -> Result<i32> {
        std::str::from_utf8(t.text)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| self.error(t, "invalid integer"))
    }

    /// Numbers may carry stray dots, like `1.5.`; as in C's `strtof`, the longest valid prefix
    /// counts.
    fn parse_number(&self, t: Token) -> Result<f32> {
        let s = std::str::from_utf8(t.text).unwrap_or_default();
        let second_dot = s.match_indices('.').nth(1).map_or(s.len(), |(i, _)| i);
        s[..second_dot].parse().map_err(|_| self.error(t, "invalid number"))
    }

    fn int(&mut self) -> Result<i32> {
        let t = self.expect(Kind::Int)?;
        self.parse_int(t)
    }

    fn maybe_int(&mut self) -> Result<Option<i32>> {
        self.maybe(Kind::Int).map(|t| self.parse_int(t)).transpose()
    }

    fn maybe_number(&mut self) -> Result<Option<f32>> {
        let saved = self.lexer.pos;
        let t = self.lexer.next();
        match t.kind {
            Kind::Int | Kind::Float => self.parse_number(t).map(Some),
            _ => {
                self.lexer.pos = saved;
                Ok(None)
            }
        }
    }

    fn number(&mut self) -> Result<f32> {
        let t = self.lexer.next();
        match t.kind {
            Kind::Int | Kind::Float => self.parse_number(t),
            kind => Err(self.error(t, format!("expected a number, found {}", kind.describe()))),
        }
    }

    /// `NAME:value`, like `FPS:25`.
    fn maybe_named(&mut self, name: &str) -> Result<Option<f32>> {
        if !self.maybe_keyword(name) {
            return Ok(None);
        }
        self.expect(Kind::Colon)?;
        self.number().map(Some)
    }

    fn flags(&mut self) -> Result<AnimationFlags> {
        let t = self.expect(Kind::Keyword)?;
        // Some scripts write `M:` where `M.` is meant.
        self.maybe(Kind::Colon);
        Ok(AnimationFlags::parse(&string(t)))
    }

    /// Steps over a block whose contents are not kept, up to its closing brace.
    fn skip_block(&mut self) {
        loop {
            match self.lexer.next().kind {
                Kind::RBrace | Kind::Eof => break,
                _ => {}
            }
        }
    }

    /// Whether the next token closes the current block, consuming it. The end of the file closes
    /// every block, since scripts often lack their last braces.
    fn block_ends(&mut self) -> bool {
        self.maybe(Kind::RBrace).is_some() || self.maybe(Kind::Eof).is_some()
    }

    fn script(&mut self) -> Result<ModelScript> {
        let mut script = ModelScript::default();

        let t = self.expect(Kind::Keyword)?;
        if !is(t, "Model") {
            return Err(self.error(t, "expected `Model`"));
        }
        let _name = self.string()?;
        self.expect(Kind::LBrace)?;

        while !self.block_ends() {
            let t = self.expect(Kind::Keyword)?;
            if is(t, "meshAndTree") {
                script.skeleton = Some(Skeleton {
                    name: self.string()?,
                    disable_mesh: self.maybe_keyword("DONT_USE_MESH"),
                });
            } else if is(t, "registerMesh") {
                script.meshes.push(self.string()?);
            } else if is(t, "aniEnum") {
                self.ani_enum(&mut script)?;
            } else {
                warn!(
                    "{}: unexpected {:?} in the Model block, ignoring the rest",
                    self.lexer.at(t.start),
                    string(t)
                );
                break;
            }
        }

        Ok(script)
    }

    fn ani_enum(&mut self, script: &mut ModelScript) -> Result<()> {
        self.expect(Kind::LBrace)?;

        while !self.block_ends() {
            let t = self.expect(Kind::Keyword)?;
            if is(t, "ani") {
                script.animations.push(self.ani()?);
            } else if is(t, "aniBlend") {
                script.blends.push(self.ani_blend()?);
            } else if is(t, "aniAlias") {
                script.aliases.push(self.ani_alias()?);
            } else if is(t, "aniComb") {
                script.combinations.push(self.ani_comb()?);
            } else if is(t, "aniDisable") {
                script.disabled_animations.push(self.string()?);
            } else if is(t, "modelTag") {
                let kind = self.string()?;
                if !kind.eq_ignore_ascii_case("DEF_HIT_LIMB") {
                    warn!("{}: unexpected model tag {kind:?}", self.lexer.at(t.start));
                }
                script.hit_limbs.push(self.string()?);
            } else {
                warn!(
                    "{}: unexpected {:?} in aniEnum, skipping the line",
                    self.lexer.at(t.start),
                    string(t)
                );
                self.lexer.eat_while(|c| c != b'\n');
            }
        }
        Ok(())
    }

    fn ani(&mut self) -> Result<Animation> {
        let mut ani = Animation {
            name: self.string()?,
            layer: self.int()? as u32,
            next: self.string()?,
            blend_in: self.number()?,
            blend_out: self.number()?,
            flags: self.flags()?,
            model: self.string()?,
            ..Animation::default()
        };

        let t = self.expect(Kind::Keyword)?;
        if !is(t, "F") && !is(t, "R") {
            return Err(self.error(t, "expected the direction `F` or `R`"));
        }
        ani.direction = Direction::parse(&string(t));
        ani.first_frame = self.int()?;
        ani.last_frame = self.int()?;
        ani.fps = self.maybe_named("FPS")?.unwrap_or(25.0);
        ani.collision_volume_scale = self.maybe_named("CVS")?.unwrap_or(1.0);

        // The braces around the events are optional; without them the events simply follow.
        self.maybe(Kind::LBrace);
        self.events(&mut ani)?;
        Ok(ani)
    }

    /// Reads events until something that is not one. With braces that is the closing brace; without
    /// them it is the next statement, which is left for the caller.
    fn events(&mut self, ani: &mut Animation) -> Result<()> {
        while !self.block_ends() {
            let saved = self.lexer.pos;
            let t = self.expect(Kind::Keyword)?;
            if is(t, "*eventTag") {
                let frame = self.maybe_int()?.unwrap_or(0);
                let kind = self.string()?;
                let a = self.maybe_string();
                let b = self.maybe_string();
                let attached = self.maybe_keyword("ATTACH");
                ani.events.push(EventTag::new(frame, &kind, a, b, attached));
            } else if is(t, "*eventSFX") {
                ani.sounds.push(self.sound()?);
            } else if is(t, "*eventSFXGrnd") {
                ani.ground_sounds.push(self.sound()?);
            } else if is(t, "*eventPFX") {
                let frame = self.int()?;
                let index = self.maybe_int()?.unwrap_or(0);
                let name = self.string()?;
                let position = self.string()?;
                // Some scripts quote it: "ATTACH".
                let attached = self.maybe_keyword("ATTACH") || self.maybe(Kind::String).is_some();
                ani.particles.push(ParticleEffect { frame, index, name, position, attached });
            } else if is(t, "*eventPFXStop") {
                ani.particle_stops
                    .push(ParticleEffectStop { frame: self.int()?, index: self.int()? });
            } else if is(t, "*eventMMStartAni") {
                ani.morphs.push(MorphAnimation {
                    frame: self.int()?,
                    animation: self.string()?,
                    node: self.maybe_string().unwrap_or_default(),
                });
            } else if is(t, "*eventCamTremor") {
                ani.tremors.push(CameraTremor {
                    frame: self.int()?,
                    values: [self.int()?, self.int()?, self.int()?, self.int()?],
                });
            } else {
                self.lexer.pos = saved;
                break;
            }
        }
        Ok(())
    }

    /// `*eventSFX` and `*eventSFXGrnd` share their syntax.
    fn sound(&mut self) -> Result<SoundEffect> {
        Ok(SoundEffect {
            frame: self.int()?,
            name: self.string()?,
            range: self.maybe_named("R")?.unwrap_or(1000.0),
            // The typo is common in the shipped scripts.
            empty_slot: self.maybe_keyword("EMPTY_SLOT") || self.maybe_keyword("EMTPY_SLOT"),
        })
    }

    fn ani_blend(&mut self) -> Result<AnimationBlend> {
        let name = self.string()?;
        let _layer = self.maybe_int()?;
        let blend = AnimationBlend {
            name,
            next: self.string()?,
            blend_in: self.maybe_number()?.unwrap_or(0.0),
            blend_out: self.maybe_number()?.unwrap_or(0.0),
        };

        // Some mods add flags, which mean nothing here. A keyword that is not made of flag
        // letters is the next statement.
        let saved = self.lexer.pos;
        match self.maybe(Kind::Keyword) {
            Some(t) if is_flags(t) => {
                self.maybe(Kind::Colon);
            }
            _ => self.lexer.pos = saved,
        }

        if self.maybe(Kind::LBrace).is_some() {
            self.skip_block();
        }
        Ok(blend)
    }

    fn ani_alias(&mut self) -> Result<AnimationAlias> {
        Ok(AnimationAlias {
            name: self.string()?,
            layer: self.int()? as u32,
            next: self.string()?,
            blend_in: self.number()?,
            blend_out: self.number()?,
            flags: self.flags()?,
            alias: self.string()?,
            direction: if self.maybe_keyword("R") {
                Direction::Backward
            } else {
                self.maybe_keyword("F");
                Direction::Forward
            },
        })
    }

    fn ani_comb(&mut self) -> Result<AnimationCombination> {
        let comb = AnimationCombination {
            name: self.string()?,
            layer: self.int()? as u32,
            next: self.string()?,
            blend_in: self.number()?,
            blend_out: self.number()?,
            flags: self.flags()?,
            model: self.string()?,
            last_frame: self.int()?,
        };
        if self.maybe(Kind::LBrace).is_some() {
            self.skip_block();
        }
        Ok(comb)
    }
}

pub(super) fn parse(src: &[u8]) -> Result<ModelScript> {
    Parser { lexer: Lexer { src, pos: 0, last: None } }.script()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Kind, String)> {
        let mut lexer = Lexer { src: src.as_bytes(), pos: 0, last: None };
        let mut out = Vec::new();
        loop {
            let t = lexer.next();
            if t.kind == Kind::Eof {
                return out;
            }
            out.push((t.kind, string(t)));
        }
    }

    #[test]
    fn parentheses_and_comments_are_ignored() {
        use Kind::*;
        assert_eq!(
            kinds("ani(\"A\" 1 -2.5 M. FPS:25) // comment\n{}"),
            [
                (Keyword, "ani".into()),
                (String, "A".into()),
                (Int, "1".into()),
                (Float, "-2.5".into()),
                (Keyword, "M.".into()),
                (Keyword, "FPS".into()),
                (Colon, ":".into()),
                (Int, "25".into()),
                (LBrace, "{".into()),
                (RBrace, "}".into()),
            ]
        );
    }

    #[test]
    fn unclosed_strings_end_at_the_line() {
        assert_eq!(
            kinds("\"A.ASC)\r\n\"B\""),
            [(Kind::String, "A.ASC".into()), (Kind::String, "B".into())]
        );
        assert_eq!(kinds("\"BODY(1)\""), [(Kind::String, "BODY(1)".into())]);
    }

    #[test]
    fn a_comment_on_the_last_line_ends_the_file() {
        assert!(kinds("// no newline").is_empty());
    }

    #[test]
    fn errors_point_at_the_token() {
        let err = parse(b"Model(\"M\") {\n  aniEnum {\n    ani(\"A\" x)\n  }\n}").unwrap_err();
        assert!(err.to_string().contains("line 3, column 13"), "{err}");
    }

    #[test]
    fn numbers_keep_their_longest_valid_prefix() {
        let src = b"Model(\"M\") { aniEnum { aniBlend(\"A\" \"B\" 1.5.0 2) } }";
        let script = parse(src).unwrap();
        assert_eq!(script.blends[0].blend_in, 1.5);
        assert_eq!(script.blends[0].blend_out, 2.0);
    }
}
