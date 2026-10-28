//! Compiled model scripts (`.MSB`): the same statements as the source, one chunk each, in source
//! order. Events follow the animation they belong to.

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
use crate::Result;
use crate::io::Reader;
use crate::text;

const CHUNK_MESH_AND_TREE: u16 = 0xF300;
const CHUNK_REGISTER_MESH: u16 = 0xF400;
const CHUNK_ANIMATION: u16 = 0xF520;
const CHUNK_ALIAS: u16 = 0xF530;
const CHUNK_BLEND: u16 = 0xF540;
const CHUNK_COMBINATION: u16 = 0xF570;
const CHUNK_DISABLE: u16 = 0xF580;
const CHUNK_MODEL_TAG: u16 = 0xF590;
const CHUNK_EVENT_SFX: u16 = 0xF5A1;
const CHUNK_EVENT_SFX_GROUND: u16 = 0xF5A2;
const CHUNK_EVENT_TAG: u16 = 0xF5A3;
const CHUNK_EVENT_PFX: u16 = 0xF5A4;
const CHUNK_EVENT_PFX_STOP: u16 = 0xF5A5;
const CHUNK_EVENT_MORPH: u16 = 0xF5A9;
const CHUNK_EVENT_TREMOR: u16 = 0xF5AA;
const CHUNK_END: u16 = 0xFFFF;

/// Every chunk of a compiled script lies in `0xF000..`. ZenKit also accepts `0xD000`, the model
/// mesh header, though no such file is known.
pub(super) fn is_chunk_id(id: u16) -> bool {
    id >= 0xF000 || id == 0xD000
}

fn string(r: &mut Reader) -> String {
    text::latin1(r.line(&[]))
}

// The struct literals below read their fields straight from the chunk. Rust evaluates a struct
// literal's fields in the order they are written, so that order is the file's.
pub(super) fn read(data: &[u8]) -> Result<ModelScript> {
    let mut script = ModelScript::default();
    let mut r = Reader::new(data);

    while !r.is_eof() {
        let mut chunk = r.chunk()?;
        let c = &mut chunk.data;
        match chunk.id {
            CHUNK_MESH_AND_TREE => {
                let disable_mesh = c.u32()? != 0;
                script.skeleton = Some(Skeleton { name: string(c), disable_mesh });
            }
            CHUNK_REGISTER_MESH => script.meshes.push(string(c)),
            CHUNK_ANIMATION => script.animations.push(Animation {
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
                ..Animation::default()
            }),
            CHUNK_ALIAS => script.aliases.push(AnimationAlias {
                name: string(c),
                layer: c.u32()?,
                next: string(c),
                blend_in: c.f32()?,
                blend_out: c.f32()?,
                flags: AnimationFlags::parse(&string(c)),
                alias: string(c),
                direction: Direction::parse(&string(c)),
            }),
            CHUNK_BLEND => script.blends.push(AnimationBlend {
                name: string(c),
                next: string(c),
                blend_in: c.f32()?,
                blend_out: c.f32()?,
            }),
            CHUNK_COMBINATION => script.combinations.push(AnimationCombination {
                name: string(c),
                layer: c.u32()?,
                next: string(c),
                blend_in: c.f32()?,
                blend_out: c.f32()?,
                flags: AnimationFlags::parse(&string(c)),
                model: string(c),
                last_frame: c.i32()?,
            }),
            CHUNK_DISABLE => script.disabled_animations.push(string(c)),
            CHUNK_MODEL_TAG => {
                let _frame = c.i32()?;
                let kind = string(c);
                if !kind.eq_ignore_ascii_case("DEF_HIT_LIMB")
                    && !kind.eq_ignore_ascii_case("HIT_LIMB")
                {
                    log::warn!("{FORMAT}: unexpected model tag {kind:?}");
                }
                script.hit_limbs.push(string(c));
            }
            CHUNK_END => break,
            id if (CHUNK_EVENT_SFX..=CHUNK_EVENT_TREMOR).contains(&id) => {
                let Some(animation) = script.animations.last_mut() else {
                    log::warn!("{FORMAT}: dropping event {id:#06x} that precedes every animation");
                    continue;
                };
                read_event(id, c, animation)?;
            }
            id => log::debug!("{FORMAT}: skipping chunk {id:#06x}"),
        }
    }

    Ok(script)
}

fn read_event(id: u16, c: &mut Reader, animation: &mut Animation) -> Result<()> {
    match id {
        CHUNK_EVENT_SFX | CHUNK_EVENT_SFX_GROUND => {
            let sound = SoundEffect {
                frame: c.i32()?,
                name: string(c),
                range: c.f32()?,
                empty_slot: c.u32()? != 0,
            };
            match id {
                CHUNK_EVENT_SFX => animation.sounds.push(sound),
                _ => animation.ground_sounds.push(sound),
            }
        }
        CHUNK_EVENT_TAG => {
            // The compiler writes four argument lines, empty when unused. No event takes more
            // than two.
            let frame = c.i32()?;
            let kind = string(c);
            let mut arg = || (!c.is_eof()).then(|| string(c));
            let (a, b) = (arg(), arg());
            animation.events.push(EventTag::new(frame, &kind, a, b, false));
        }
        CHUNK_EVENT_PFX => animation.particles.push(ParticleEffect {
            frame: c.i32()?,
            index: c.i32()?,
            name: string(c),
            position: string(c),
            attached: c.u32()? != 0,
        }),
        CHUNK_EVENT_PFX_STOP => {
            animation.particle_stops.push(ParticleEffectStop { frame: c.i32()?, index: c.i32()? })
        }
        CHUNK_EVENT_MORPH => {
            // Followed by two floats nobody has identified yet.
            animation.morphs.push(MorphAnimation {
                frame: c.i32()?,
                animation: string(c),
                node: string(c),
            });
        }
        CHUNK_EVENT_TREMOR => animation.tremors.push(CameraTremor {
            frame: c.i32()?,
            values: [c.i32()?, c.i32()?, c.i32()?, c.i32()?],
        }),
        id => log::debug!("{FORMAT}: skipping event chunk {id:#06x}"),
    }
    Ok(())
}
