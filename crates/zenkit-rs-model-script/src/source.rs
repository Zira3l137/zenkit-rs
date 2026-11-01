//! Source model scripts (`.MDS`).
//!
//! The syntax looks like function calls, `ani("S_RUN" 1 "S_RUN" 0.1 0.1 M. "HUM_RUN.ASC" F 1 20)`,
//! but the engine ignores parentheses entirely: a script is just a stream of keywords, strings and
//! numbers, with braces for blocks. Handwritten scripts, mods' especially, are full of missing
//! quotes, parentheses and braces that the engine shrugs off, so this parser follows ZenKit's in
//! accepting them.

use tracing::warn;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::WHITESPACE;
use zenkit_rs_core::text;

use super::Animation;
use super::AnimationAlias;
use super::AnimationBlend;
use super::AnimationCombination;
use super::AnimationFlags;
use super::CameraTremor;
use super::Direction;
use super::Event;
use super::EventTag;
use super::EventType;
use super::FORMAT;
use super::ModelScript;
use super::MorphAnimation;
use super::ParticleEffect;
use super::ParticleEffectStop;
use super::Skeleton;
use super::SoundEffect;
use super::Statement;

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

    /// The flags and their text as written. Some scripts write `M:` where `M.` is meant; the
    /// colon is kept with the text, as compiled scripts keep it.
    fn flags(&mut self) -> Result<(AnimationFlags, String)> {
        let mut text = string(self.expect(Kind::Keyword)?);
        if self.maybe(Kind::Colon).is_some() {
            text.push(':');
        }
        Ok((AnimationFlags::parse(&text), text))
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
        script.name = self.string()?;
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
            let statement = if is(t, "ani") {
                Statement::Animation(self.ani()?)
            } else if is(t, "aniBlend") {
                Statement::Blend(self.ani_blend()?)
            } else if is(t, "aniAlias") {
                Statement::Alias(self.ani_alias()?)
            } else if is(t, "aniComb") {
                Statement::Combination(self.ani_comb()?)
            } else if is(t, "aniDisable") {
                Statement::Disable(self.string()?)
            } else if is(t, "modelTag") {
                Statement::ModelTag(self.tag(t)?)
            } else {
                warn!(
                    "{}: unexpected {:?} in aniEnum, skipping the line",
                    self.lexer.at(t.start),
                    string(t)
                );
                self.lexer.eat_while(|c| c != b'\n');
                continue;
            };
            script.statements.push(statement);
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
            flags: AnimationFlags::empty(),
            flags_text: self.flags()?.1,
            model: self.string()?,
            ..Animation::default()
        };
        ani.flags = AnimationFlags::parse(&ani.flags_text);

        let t = self.expect(Kind::Keyword)?;
        if !is(t, "F") && !is(t, "R") {
            return Err(self.error(t, "expected the direction `F` or `R`"));
        }
        ani.direction = Direction::parse(&string(t));
        ani.first_frame = self.int()?;
        ani.last_frame = self.int()?;
        ani.fps = 25.0;
        ani.collision_volume_scale = 1.0;
        // Any of them, in any order.
        loop {
            if let Some(fps) = self.maybe_named("FPS")? {
                ani.fps = fps;
            } else if let Some(speed) = self.maybe_named("SPD")? {
                ani.speed = speed;
            } else if let Some(cvs) = self.maybe_named("CVS")? {
                ani.collision_volume_scale = cvs;
            } else {
                break;
            }
        }
        ani.events = self.events()?;
        Ok(ani)
    }

    /// Reads the events that follow a statement. They are usually in braces, but scripts also
    /// leave the braces out, or the closing one, and then the events end at the next statement.
    fn events(&mut self) -> Result<Vec<Event>> {
        let braced = self.maybe(Kind::LBrace).is_some();
        let mut events = Vec::new();
        loop {
            if braced && self.block_ends() {
                break;
            }
            let saved = self.lexer.pos;
            let t = self.lexer.next();
            if t.kind != Kind::Keyword {
                if braced {
                    return Err(
                        self.error(t, format!("expected an event, found {}", t.kind.describe()))
                    );
                }
                self.lexer.pos = saved;
                break;
            }
            let event = if is(t, "*eventTag") {
                Event::Tag(self.tag(t)?)
            } else if is(t, "*eventSFX") {
                Event::Sound(self.sound()?)
            } else if is(t, "*eventSFXGrnd") {
                Event::GroundSound(self.sound()?)
            } else if is(t, "*eventPFX") {
                let frame = self.int()?;
                let index = self.maybe_int()?.unwrap_or(0);
                let name = self.string()?;
                let position = self.string()?;
                // Some scripts quote it: "ATTACH".
                let attached = self.maybe_keyword("ATTACH") || self.maybe(Kind::String).is_some();
                Event::Particle(ParticleEffect { frame, index, name, position, attached })
            } else if is(t, "*eventPFXStop") {
                Event::ParticleStop(ParticleEffectStop { frame: self.int()?, index: self.int()? })
            } else if is(t, "*eventMMStartAni") {
                let mut morph = MorphAnimation {
                    frame: self.int()?,
                    animation: self.string()?,
                    node: self.maybe_string().unwrap_or_default(),
                    ..MorphAnimation::default()
                };
                loop {
                    if let Some(intensity) = self.maybe_named("I")? {
                        morph.intensity = intensity;
                    } else if let Some(hold_time) = self.maybe_named("H")? {
                        morph.hold_time = hold_time;
                    } else {
                        break;
                    }
                }
                Event::Morph(morph)
            } else if is(t, "*eventCamTremor") {
                Event::Tremor(CameraTremor {
                    frame: self.int()?,
                    range: self.int()?,
                    duration: self.int()?,
                    min_amplifier: self.int()?,
                    max_amplifier: self.int()?,
                })
            } else if t.text.first() == Some(&b'*') {
                warn!("{}: skipping unknown event {:?}", self.lexer.at(t.start), string(t));
                self.lexer.eat_while(|c| c != b'\n');
                continue;
            } else {
                // The next statement. In braces, the closing brace is missing, as in some mods'
                // scripts, which the engine accepts.
                self.lexer.pos = saved;
                break;
            };
            events.push(event);
        }
        Ok(events)
    }

    /// The arguments of `*eventTag` and `modelTag`: an optional frame, the event's name, up to
    /// four arguments and `ATTACH`.
    fn tag(&mut self, keyword: Token) -> Result<EventTag> {
        let frame = self.maybe_int()?.unwrap_or(0);
        let name = self.string()?;
        if EventType::from_name(&name) == EventType::Unknown {
            warn!("{}: unknown event type {name:?}", self.lexer.at(keyword.start));
        }
        let mut arguments = Vec::new();
        while arguments.len() < 4
            && let Some(argument) = self.maybe_string()
        {
            arguments.push(argument);
        }
        let attached = self.maybe_keyword("ATTACH");
        Ok(EventTag { frame, name, arguments, attached })
    }

    /// `*eventSFX` and `*eventSFXGrnd` share their syntax.
    fn sound(&mut self) -> Result<SoundEffect> {
        let mut sound =
            SoundEffect { frame: self.int()?, name: self.string()?, ..SoundEffect::default() };
        if let Some(range) = self.maybe_named("R")? {
            sound.range = range;
        }
        // The typo is common in the shipped scripts.
        sound.empty_slot = self.maybe_keyword("EMPTY_SLOT") || self.maybe_keyword("EMTPY_SLOT");
        Ok(sound)
    }

    fn ani_blend(&mut self) -> Result<AnimationBlend> {
        let name = self.string()?;
        let _layer = self.maybe_int()?;
        let mut blend = AnimationBlend {
            name,
            next: self.string()?,
            blend_in: self.maybe_number()?.unwrap_or(0.0),
            blend_out: self.maybe_number()?.unwrap_or(0.0),
            events: Vec::new(),
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

        blend.events = self.events()?;
        Ok(blend)
    }

    fn ani_alias(&mut self) -> Result<AnimationAlias> {
        let mut alias = AnimationAlias {
            name: self.string()?,
            layer: self.int()? as u32,
            next: self.string()?,
            blend_in: self.number()?,
            blend_out: self.number()?,
            flags: AnimationFlags::empty(),
            flags_text: self.flags()?.1,
            alias: self.string()?,
            direction: if self.maybe_keyword("R") {
                Direction::Backward
            } else {
                self.maybe_keyword("F");
                Direction::Forward
            },
            events: Vec::new(),
        };
        alias.flags = AnimationFlags::parse(&alias.flags_text);
        alias.events = self.events()?;
        Ok(alias)
    }

    fn ani_comb(&mut self) -> Result<AnimationCombination> {
        let mut comb = AnimationCombination {
            name: self.string()?,
            layer: self.int()? as u32,
            next: self.string()?,
            blend_in: self.number()?,
            blend_out: self.number()?,
            flags: AnimationFlags::empty(),
            flags_text: self.flags()?.1,
            model: self.string()?,
            last_frame: self.int()?,
            events: Vec::new(),
        };
        comb.flags = AnimationFlags::parse(&comb.flags_text);
        comb.events = self.events()?;
        Ok(comb)
    }
}

pub(super) fn parse(src: &[u8]) -> Result<ModelScript> {
    Parser { lexer: Lexer { src, pos: 0, last: None } }.script()
}

/// Writes a script as source. Texts and numbers are checked as they are written; the first
/// problem is kept and returned at the end.
pub(super) fn write(script: &ModelScript) -> Result<Vec<u8>> {
    let mut w = Source { out: Vec::new(), error: None };

    let name = w.checked(&script.name);
    w.line(0, &format!("Model ({name})"));
    w.line(0, "{");
    if let Some(skeleton) = &script.skeleton {
        let flag = if skeleton.disable_mesh { " DONT_USE_MESH" } else { "" };
        let name = w.checked(&skeleton.name);
        w.line(1, &format!("meshAndTree ({name}{flag})"));
    }
    if !script.meshes.is_empty() {
        w.line(0, "");
        for mesh in &script.meshes {
            let mesh = w.checked(mesh);
            w.line(1, &format!("registerMesh ({mesh})"));
        }
    }
    w.line(0, "");
    w.line(1, "aniEnum");
    w.line(1, "{");
    for statement in &script.statements {
        let line = w.statement(statement);
        w.line(2, &line);
        let events = statement.events();
        if !events.is_empty() {
            w.line(2, "{");
            for event in events {
                let line = w.event(event);
                w.line(3, &line);
            }
            w.line(2, "}");
        }
    }
    w.line(1, "}");
    w.line(0, "}");

    match w.error {
        Some(error) => Err(error),
        None => Ok(w.out),
    }
}

struct Source {
    out: Vec<u8>,
    error: Option<Error>,
}

impl Source {
    fn fail(&mut self, reason: String) {
        self.error.get_or_insert(Error::unwritable(FORMAT, reason));
    }

    fn line(&mut self, depth: usize, text: &str) {
        if !text.is_empty() {
            self.out.extend(std::iter::repeat_n(b'\t', depth));
        }
        match text::to_latin1(text) {
            Some(bytes) => self.out.extend(bytes),
            None => self.fail(format!("{text:?} has characters beyond Latin-1")),
        }
        self.out.extend(b"\r\n");
    }

    /// A quoted string, which ends at a quote or a line break, so cannot hold either.
    fn string(&self, text: &str) -> String {
        format!("\"{text}\"")
    }

    fn checked(&mut self, text: &str) -> String {
        if text.contains(['"', '\r', '\n', '\0']) {
            self.fail(format!("{text:?} has a quote or a line break"));
        }
        self.string(text)
    }

    /// A number as scripts write it, shortest first, with a decimal point when it has no
    /// fraction, like `0.0` or `0.25`.
    fn float(&mut self, value: f32) -> String {
        let text = self.named_float(value);
        if text.contains('.') { text } else { format!("{text}.0") }
    }

    /// A number after a name, like `FPS:25`, which leaves out a fraction that is zero.
    fn named_float(&mut self, value: f32) -> String {
        if !value.is_finite() {
            self.fail(format!("{value} is not a number scripts can hold"));
        }
        value.to_string()
    }

    fn statement(&mut self, statement: &Statement) -> String {
        match statement {
            Statement::Animation(a) => {
                let mut line = format!(
                    "ani ({} {} {} {} {} {} {} {} {} {}",
                    self.checked(&a.name),
                    a.layer,
                    self.checked(&a.next),
                    self.float(a.blend_in),
                    self.float(a.blend_out),
                    a.flags.spelled(&a.flags_text),
                    self.checked(&a.model),
                    a.direction.to_text(),
                    a.first_frame,
                    a.last_frame,
                );
                if a.fps != 25.0 {
                    line += &format!(" FPS:{}", self.named_float(a.fps));
                }
                if a.speed != 0.0 {
                    line += &format!(" SPD:{}", self.named_float(a.speed));
                }
                if a.collision_volume_scale != 1.0 {
                    line += &format!(" CVS:{}", self.named_float(a.collision_volume_scale));
                }
                line + ")"
            }
            Statement::Alias(a) => format!(
                "aniAlias ({} {} {} {} {} {} {} {})",
                self.checked(&a.name),
                a.layer,
                self.checked(&a.next),
                self.float(a.blend_in),
                self.float(a.blend_out),
                a.flags.spelled(&a.flags_text),
                self.checked(&a.alias),
                a.direction.to_text(),
            ),
            Statement::Blend(b) => format!(
                "aniBlend ({} {} {} {})",
                self.checked(&b.name),
                self.checked(&b.next),
                self.float(b.blend_in),
                self.float(b.blend_out),
            ),
            Statement::Combination(c) => format!(
                "aniComb ({} {} {} {} {} {} {} {})",
                self.checked(&c.name),
                c.layer,
                self.checked(&c.next),
                self.float(c.blend_in),
                self.float(c.blend_out),
                c.flags.spelled(&c.flags_text),
                self.checked(&c.model),
                c.last_frame,
            ),
            Statement::Disable(name) => format!("aniDisable ({})", self.checked(name)),
            Statement::ModelTag(tag) => {
                // The shipped scripts leave out a model tag's frame, which is always zero there.
                let frame = if tag.frame == 0 { String::new() } else { format!("{} ", tag.frame) };
                format!("modelTag ({frame}{})", self.tag_arguments(tag))
            }
            Statement::Unknown(raw) => {
                let id = raw.id;
                self.fail(format!("compiled chunk {id:#06x} has no source form"));
                String::new()
            }
        }
    }

    /// A tag's name, arguments and `ATTACH`.
    fn tag_arguments(&mut self, tag: &EventTag) -> String {
        let mut parts = vec![self.checked(&tag.name)];
        parts.extend(tag.arguments.iter().map(|a| self.checked(a)));
        if tag.attached {
            parts.push("ATTACH".into());
        }
        parts.join(" ")
    }

    fn event(&mut self, event: &Event) -> String {
        let sound = |w: &mut Self, keyword: &str, s: &SoundEffect| {
            let mut line = format!("{keyword} ({} {}", s.frame, w.checked(&s.name));
            if s.range != 1000.0 {
                line += &format!(" R:{}", w.named_float(s.range));
            }
            if s.empty_slot {
                line += " EMPTY_SLOT";
            }
            line + ")"
        };
        match event {
            Event::Tag(tag) => format!("*eventTag ({} {})", tag.frame, self.tag_arguments(tag)),
            Event::Sound(s) => sound(self, "*eventSFX", s),
            Event::GroundSound(s) => sound(self, "*eventSFXGrnd", s),
            Event::Particle(p) => {
                let index = if p.index == 0 { String::new() } else { format!(" {}", p.index) };
                let attach = if p.attached { " ATTACH" } else { "" };
                let (name, position) = (self.checked(&p.name), self.checked(&p.position));
                format!("*eventPFX ({}{index} {name} {position}{attach})", p.frame)
            }
            Event::ParticleStop(p) => format!("*eventPFXStop ({} {})", p.frame, p.index),
            Event::Morph(m) => {
                let mut line =
                    format!("*eventMMStartAni ({} {}", m.frame, self.checked(&m.animation));
                if !m.node.is_empty() {
                    line += &format!(" {}", self.checked(&m.node));
                }
                if m.intensity != 0.0 {
                    line += &format!(" I:{}", self.named_float(m.intensity));
                }
                if m.hold_time != 0.0 {
                    line += &format!(" H:{}", self.named_float(m.hold_time));
                }
                line + ")"
            }
            Event::Tremor(t) => format!(
                "*eventCamTremor ({} {} {} {} {})",
                t.frame, t.range, t.duration, t.min_amplifier, t.max_amplifier
            ),
            Event::Unknown(raw) => {
                let id = raw.id;
                self.fail(format!("compiled event chunk {id:#06x} has no source form"));
                String::new()
            }
        }
    }
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
        let blend = script.blends().next().unwrap();
        assert_eq!((blend.blend_in, blend.blend_out), (1.5, 2.0));
    }
}
