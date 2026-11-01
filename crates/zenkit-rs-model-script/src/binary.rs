//! Compiled model scripts (`.MSB`): the same statements as the source, one chunk each, in source
//! order. An animation's events follow it, between an events chunk holding their count and an
//! end-of-events chunk.

use tracing::warn;
use zenkit_rs_core::Date;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
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
                script.source = Some(ScriptSource { date, path: string(c) });
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
                script.statements.push(read_statement(id, c)?);
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
            flags: AnimationFlags::parse(&string(c)),
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
            flags: AnimationFlags::parse(&string(c)),
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
            flags: AnimationFlags::parse(&string(c)),
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
        Ok::<_, zenkit_rs_core::Error>(SoundEffect {
            frame: c.i32()?,
            name: string(c),
            range: c.f32()?,
            empty_slot: c.u32()? != 0,
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
            attached: c.u32()? != 0,
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
