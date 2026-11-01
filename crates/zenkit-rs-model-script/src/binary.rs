//! Compiled model scripts (`.MSB`): the same statements as the source, one chunk each, in source
//! order. An animation's events follow it, between an events chunk holding their count and an
//! end-of-events chunk.

use tracing::warn;
use zenkit_rs_core::Date;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::Writer;
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
use super::RawChunk;
use super::ScriptSource;
use super::Skeleton;
use super::SoundEffect;
use super::Statement;

pub(super) const CHUNK_ROOT: u16 = 0xF000;
pub(super) const CHUNK_SOURCE: u16 = 0xF100;
pub(super) const CHUNK_MODEL: u16 = 0xF200;
pub(super) const CHUNK_MODEL_END: u16 = 0xF2FF;
pub(super) const CHUNK_MESH_AND_TREE: u16 = 0xF300;
pub(super) const CHUNK_REGISTER_MESH: u16 = 0xF400;
pub(super) const CHUNK_ANI_ENUM: u16 = 0xF500;
pub(super) const CHUNK_ANI_ENUM_END: u16 = 0xF5FF;
pub(super) const CHUNK_ANIMATION: u16 = 0xF520;
pub(super) const CHUNK_ALIAS: u16 = 0xF530;
pub(super) const CHUNK_BLEND: u16 = 0xF540;
pub(super) const CHUNK_COMBINATION: u16 = 0xF570;
pub(super) const CHUNK_DISABLE: u16 = 0xF580;
pub(super) const CHUNK_MODEL_TAG: u16 = 0xF590;
pub(super) const CHUNK_EVENTS: u16 = 0xF5A0;
pub(super) const CHUNK_EVENT_SFX: u16 = 0xF5A1;
pub(super) const CHUNK_EVENT_SFX_GROUND: u16 = 0xF5A2;
pub(super) const CHUNK_EVENT_TAG: u16 = 0xF5A3;
pub(super) const CHUNK_EVENT_PFX: u16 = 0xF5A4;
pub(super) const CHUNK_EVENT_PFX_STOP: u16 = 0xF5A5;
pub(super) const CHUNK_EVENT_MORPH: u16 = 0xF5A9;
pub(super) const CHUNK_EVENT_TREMOR: u16 = 0xF5AA;
pub(super) const CHUNK_EVENTS_END: u16 = 0xF5AF;
pub(super) const CHUNK_END: u16 = 0xFFFF;

/// Every chunk of a compiled script lies in `0xF000..`. ZenKit also accepts `0xD000`, the model
/// mesh header, though no such file is known.
pub(super) fn is_chunk_id(id: u16) -> bool {
    id >= 0xF000 || id == 0xD000
}

fn string(r: &mut Reader) -> String {
    text::latin1(r.line(&[]))
}

/// The four argument lines of a tag, without the empty ones at the end.
fn arguments(c: &mut Reader) -> Vec<String> {
    let mut arguments: Vec<_> = (0..4).map_while(|_| (!c.is_eof()).then(|| string(c))).collect();
    while arguments.last().is_some_and(String::is_empty) {
        arguments.pop();
    }
    arguments
}

fn tag(c: &mut Reader) -> Result<EventTag> {
    let frame = c.i32()?;
    let name = string(c);
    if EventType::from_name(&name) == EventType::Unknown {
        warn!("unknown event type {name:?}");
    }
    Ok(EventTag { frame, name, arguments: arguments(c), attached: false })
}

// The struct literals below read their fields straight from the chunk. Rust evaluates a struct
// literal's fields in the order they are written, so that order is the file's.
/// Sets a statement's flags from their text, which the struct literals read in file order.
fn parse_flags(mut statement: Statement) -> Statement {
    match &mut statement {
        Statement::Animation(a) => a.flags = AnimationFlags::parse(&a.flags_text),
        Statement::Alias(a) => a.flags = AnimationFlags::parse(&a.flags_text),
        Statement::Combination(c) => c.flags = AnimationFlags::parse(&c.flags_text),
        _ => {}
    }
    statement
}

pub(super) fn read(data: &[u8]) -> Result<ModelScript> {
    let mut script = ModelScript::default();
    let mut r = Reader::new(data);
    // Whether the chunks being read are the events of the last statement.
    let mut in_events = false;

    while !r.is_eof() {
        let mut chunk = r.chunk()?;
        let c = &mut chunk.data;
        match chunk.id {
            CHUNK_ROOT | CHUNK_MODEL | CHUNK_MODEL_END | CHUNK_ANI_ENUM | CHUNK_ANI_ENUM_END => {}
            CHUNK_SOURCE => {
                let date = Date::read(c)?;
                let path = string(c);
                let file = path.rsplit(['\\', '/']).next().unwrap_or_default();
                script.name = file.rsplit_once('.').map_or(file, |(stem, _)| stem).to_owned();
                script.source = Some(ScriptSource { date, path });
            }
            CHUNK_MESH_AND_TREE => {
                let disable_mesh = c.u32()? != 0;
                script.skeleton = Some(Skeleton { name: string(c), disable_mesh });
            }
            CHUNK_REGISTER_MESH => script.meshes.push(string(c)),
            CHUNK_EVENTS => {
                let holds_events = script.statements.last_mut().and_then(Statement::events_mut);
                in_events = holds_events.is_some();
                if !in_events {
                    warn!("dropping events that follow no animation");
                }
            }
            CHUNK_EVENTS_END => in_events = false,
            CHUNK_END => break,
            id if (CHUNK_EVENTS..CHUNK_EVENTS_END).contains(&id) => {
                let events = script.statements.last_mut().and_then(Statement::events_mut);
                match events {
                    Some(events) if in_events => events.push(read_event(id, c)?),
                    _ => warn!("dropping event {id:#06x} outside the events of an animation"),
                }
            }
            id => {
                in_events = false;
                script.statements.push(parse_flags(read_statement(id, c)?));
            }
        }
    }

    Ok(script)
}

fn read_statement(id: u16, c: &mut Reader) -> Result<Statement> {
    Ok(match id {
        CHUNK_ANIMATION => Statement::Animation(Animation {
            name: string(c),
            layer: c.u32()?,
            next: string(c),
            blend_in: c.f32()?,
            blend_out: c.f32()?,
            flags: AnimationFlags::empty(),
            flags_text: string(c),
            model: string(c),
            direction: Direction::parse(&string(c)),
            first_frame: c.i32()?,
            last_frame: c.i32()?,
            fps: c.f32()?,
            speed: c.f32()?,
            collision_volume_scale: c.f32()?,
            events: Vec::new(),
        }),
        CHUNK_ALIAS => Statement::Alias(AnimationAlias {
            name: string(c),
            layer: c.u32()?,
            next: string(c),
            blend_in: c.f32()?,
            blend_out: c.f32()?,
            flags: AnimationFlags::empty(),
            flags_text: string(c),
            alias: string(c),
            direction: Direction::parse(&string(c)),
            events: Vec::new(),
        }),
        CHUNK_BLEND => Statement::Blend(AnimationBlend {
            name: string(c),
            next: string(c),
            blend_in: c.f32()?,
            blend_out: c.f32()?,
            events: Vec::new(),
        }),
        CHUNK_COMBINATION => Statement::Combination(AnimationCombination {
            name: string(c),
            layer: c.u32()?,
            next: string(c),
            blend_in: c.f32()?,
            blend_out: c.f32()?,
            flags: AnimationFlags::empty(),
            flags_text: string(c),
            model: string(c),
            last_frame: c.i32()?,
            events: Vec::new(),
        }),
        CHUNK_DISABLE => Statement::Disable(string(c)),
        CHUNK_MODEL_TAG => Statement::ModelTag(tag(c)?),
        id => Statement::Unknown(RawChunk { id, data: c.rest().to_vec() }),
    })
}

fn read_event(id: u16, c: &mut Reader) -> Result<Event> {
    let sound = |c: &mut Reader| {
        Ok::<_, Error>(SoundEffect {
            frame: c.i32()?,
            name: string(c),
            range: c.f32()?,
            // A float, like the engine's other event values.
            empty_slot: c.f32()? != 0.0,
        })
    };
    Ok(match id {
        CHUNK_EVENT_SFX => Event::Sound(sound(c)?),
        CHUNK_EVENT_SFX_GROUND => Event::GroundSound(sound(c)?),
        CHUNK_EVENT_TAG => Event::Tag(tag(c)?),
        CHUNK_EVENT_PFX => Event::Particle(ParticleEffect {
            frame: c.i32()?,
            index: c.i32()?,
            name: string(c),
            position: string(c),
            // A float, like the sound flag.
            attached: c.f32()? != 0.0,
        }),
        CHUNK_EVENT_PFX_STOP => {
            Event::ParticleStop(ParticleEffectStop { frame: c.i32()?, index: c.i32()? })
        }
        CHUNK_EVENT_MORPH => Event::Morph(MorphAnimation {
            frame: c.i32()?,
            animation: string(c),
            node: string(c),
            intensity: c.f32()?,
            hold_time: c.f32()?,
        }),
        CHUNK_EVENT_TREMOR => Event::Tremor(CameraTremor {
            frame: c.i32()?,
            range: c.i32()?,
            duration: c.i32()?,
            min_amplifier: c.i32()?,
            max_amplifier: c.i32()?,
        }),
        id => Event::Unknown(RawChunk { id, data: c.rest().to_vec() }),
    })
}

/// Writes a script compiled. Statements carry their texts, so every text is checked before
/// anything is written, by encoding it as it goes: the first failure is kept and returned at the
/// end, as the archive writer does.
pub(super) fn write(script: &ModelScript) -> Result<Vec<u8>> {
    let mut w = Out { w: Writer::new(), error: None };
    let source = script.source.clone().unwrap_or_default();
    // The directory of the source file, which the root chunk names.
    let directory = source.path.rfind('\\').map_or("", |end| &source.path[..=end]);

    w.chunk(CHUNK_ROOT, |w| {
        w.w.u32(1);
        w.line(directory);
    });
    w.chunk(CHUNK_SOURCE, |w| {
        source.date.write(&mut w.w);
        w.line(&source.path);
    });
    w.chunk(CHUNK_MODEL, |_| ());
    if let Some(skeleton) = &script.skeleton {
        w.chunk(CHUNK_MESH_AND_TREE, |w| {
            w.w.u32(skeleton.disable_mesh.into());
            w.line(&skeleton.name);
        });
    }
    for mesh in &script.meshes {
        w.chunk(CHUNK_REGISTER_MESH, |w| w.line(mesh));
    }
    w.chunk(CHUNK_ANI_ENUM, |_| ());
    for statement in &script.statements {
        write_statement(&mut w, statement);
        let events = statement.events();
        if !events.is_empty() {
            w.chunk(CHUNK_EVENTS, |w| w.w.u32(events.len() as u32));
            events.iter().for_each(|event| write_event(&mut w, event));
            w.chunk(CHUNK_EVENTS_END, |_| ());
        }
    }
    w.chunk(CHUNK_ANI_ENUM_END, |_| ());
    w.chunk(CHUNK_MODEL_END, |_| ());
    w.chunk(CHUNK_END, |_| ());

    match w.error {
        Some(error) => Err(error),
        None => Ok(w.w.into_inner()),
    }
}

struct Out {
    w: Writer,
    error: Option<Error>,
}

impl Out {
    fn chunk(&mut self, id: u16, contents: impl FnOnce(&mut Self)) {
        self.w.u16(id);
        let length_at = self.w.position();
        self.w.u32(0);
        contents(self);
        let length = self.w.position() - length_at - 4;
        self.w.patch_u32(length_at, length as u32);
    }

    fn line(&mut self, value: &str) {
        match text::to_line(FORMAT, value) {
            Ok(bytes) => self.w.line(&bytes),
            Err(error) => {
                self.error.get_or_insert(error);
            }
        }
    }

    fn tag(&mut self, tag: &EventTag) {
        if tag.arguments.len() > 4 {
            let reason =
                format!("{:?} has {} arguments, at most 4 fit", tag.name, tag.arguments.len());
            self.error.get_or_insert(Error::unwritable(FORMAT, reason));
        }
        self.w.i32(tag.frame);
        self.line(&tag.name);
        for i in 0..4 {
            self.line(tag.argument(i));
        }
    }
}

fn write_statement(w: &mut Out, statement: &Statement) {
    match statement {
        Statement::Animation(a) => w.chunk(CHUNK_ANIMATION, |w| {
            w.line(&a.name);
            w.w.u32(a.layer);
            w.line(&a.next);
            w.w.f32(a.blend_in);
            w.w.f32(a.blend_out);
            w.line(&a.flags.spelled(&a.flags_text));
            w.line(&a.model);
            w.line(a.direction.to_text());
            w.w.i32(a.first_frame);
            w.w.i32(a.last_frame);
            w.w.f32(a.fps);
            w.w.f32(a.speed);
            w.w.f32(a.collision_volume_scale);
        }),
        Statement::Alias(a) => w.chunk(CHUNK_ALIAS, |w| {
            w.line(&a.name);
            w.w.u32(a.layer);
            w.line(&a.next);
            w.w.f32(a.blend_in);
            w.w.f32(a.blend_out);
            w.line(&a.flags.spelled(&a.flags_text));
            w.line(&a.alias);
            w.line(a.direction.to_text());
        }),
        Statement::Blend(b) => w.chunk(CHUNK_BLEND, |w| {
            w.line(&b.name);
            w.line(&b.next);
            w.w.f32(b.blend_in);
            w.w.f32(b.blend_out);
        }),
        Statement::Combination(c) => w.chunk(CHUNK_COMBINATION, |w| {
            w.line(&c.name);
            w.w.u32(c.layer);
            w.line(&c.next);
            w.w.f32(c.blend_in);
            w.w.f32(c.blend_out);
            w.line(&c.flags.spelled(&c.flags_text));
            w.line(&c.model);
            w.w.i32(c.last_frame);
        }),
        Statement::Disable(name) => w.chunk(CHUNK_DISABLE, |w| w.line(name)),
        Statement::ModelTag(tag) => w.chunk(CHUNK_MODEL_TAG, |w| w.tag(tag)),
        Statement::Unknown(raw) => w.chunk(raw.id, |w| w.w.bytes(&raw.data)),
    }
}

fn write_event(w: &mut Out, event: &Event) {
    let sound = |w: &mut Out, s: &SoundEffect| {
        w.w.i32(s.frame);
        w.line(&s.name);
        w.w.f32(s.range);
        w.w.f32(if s.empty_slot { 1.0 } else { 0.0 });
    };
    match event {
        Event::Sound(s) => w.chunk(CHUNK_EVENT_SFX, |w| sound(w, s)),
        Event::GroundSound(s) => w.chunk(CHUNK_EVENT_SFX_GROUND, |w| sound(w, s)),
        Event::Tag(tag) => w.chunk(CHUNK_EVENT_TAG, |w| w.tag(tag)),
        Event::Particle(p) => w.chunk(CHUNK_EVENT_PFX, |w| {
            w.w.i32(p.frame);
            w.w.i32(p.index);
            w.line(&p.name);
            w.line(&p.position);
            w.w.f32(if p.attached { 1.0 } else { 0.0 });
        }),
        Event::ParticleStop(p) => w.chunk(CHUNK_EVENT_PFX_STOP, |w| {
            w.w.i32(p.frame);
            w.w.i32(p.index);
        }),
        Event::Morph(m) => w.chunk(CHUNK_EVENT_MORPH, |w| {
            w.w.i32(m.frame);
            w.line(&m.animation);
            w.line(&m.node);
            w.w.f32(m.intensity);
            w.w.f32(m.hold_time);
        }),
        Event::Tremor(t) => w.chunk(CHUNK_EVENT_TREMOR, |w| {
            for v in [t.frame, t.range, t.duration, t.min_amplifier, t.max_amplifier] {
                w.w.i32(v);
            }
        }),
        Event::Unknown(raw) => w.chunk(raw.id, |w| w.w.bytes(&raw.data)),
    }
}
