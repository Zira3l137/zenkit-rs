//! Skeletal animations (`.MAN`).
//!
//! Samples are compressed to six `u16`s each: a rotation stored as the quaternion's `x`, `y` and
//! `z` (its `w` follows from the quaternion having unit length), and a position quantised into
//! the range given by [`ModelAnimation::sample_position_min`] and
//! [`ModelAnimation::sample_position_scale`].

use tracing::debug;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Date;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::WHITESPACE;
use zenkit_rs_core::math::Aabb;
use zenkit_rs_core::math::Quat;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::raw_enum;
use zenkit_rs_core::text;
use zerocopy::little_endian::U16;

const CHUNK_MARKER: u16 = 0xA000;
const CHUNK_SOURCE: u16 = 0xA010;
const CHUNK_HEADER: u16 = 0xA020;
const CHUNK_EVENTS: u16 = 0xA030;
const CHUNK_SAMPLES: u16 = 0xA090;

const ROTATION_RANGE: f32 = (1 << 16) as f32 - 1.0;
const ROTATION_SCALE: f32 = (1.0 / ROTATION_RANGE) * 2.1;
const ROTATION_MID: f32 = ((1 << 15) - 1) as f32;

raw_enum! {
    pub enum AnimationEventType: u32 = Tag {
        Tag = 0,
        SoundEffect = 1,
        SoundEffectGround = 2,
        Batch = 3,
        SwapMesh = 4,
        Heading = 5,
        ParticleEffect = 6,
        ParticleEffectGround = 7,
        ParticleEffectStop = 8,
        SetMesh = 9,
        MorphMeshAnimation = 10,
        CameraTremor = 11,
    }
}

/// Something the animation triggers at a given frame, like a footstep sound.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimationEvent {
    pub kind: AnimationEventType,
    pub frame: u32,
    pub tag: String,
    pub content: [String; 4],
    pub values: [f32; 4],
    pub probability: f32,
}

/// One node's pose in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnimationSample {
    pub position: Vec3,
    pub rotation: Quat,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelAnimation {
    pub name: String,
    /// The animation the engine plays once this one ends.
    pub next: String,
    pub layer: u32,
    pub frame_count: u32,
    pub node_count: u32,
    pub fps: f32,
    pub fps_source: f32,
    pub sample_position_min: f32,
    pub sample_position_scale: f32,
    pub bbox: Aabb,
    /// Matches the checksum of the skeleton (`ModelHierarchy`) it animates.
    pub checksum: u32,
    pub source_path: String,
    /// The engine writes this from an uninitialised variable, so its values mean nothing.
    pub source_date: Date,
    /// The `ani` line of the model script this animation was compiled from.
    pub source_script: String,
    /// Frame by frame, one sample per node of [`node_indices`](Self::node_indices).
    pub samples: Vec<AnimationSample>,
    pub events: Vec<AnimationEvent>,
    /// The hierarchy nodes this animation moves, in sample order.
    pub node_indices: Vec<u32>,
}

impl ModelAnimation {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let mut anim = Self::default();

        // Animations have no end chunk; they simply stop.
        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_MARKER => {}
                CHUNK_SOURCE => {
                    anim.source_date = Date::read(c)?;
                    anim.source_path = text::latin1(c.line(&[]));
                    anim.source_script = text::latin1(c.line(&[]));
                }
                CHUNK_HEADER => {
                    let _version = c.u16()?;
                    anim.name = text::latin1(c.line(&[]));
                    anim.layer = c.u32()?;
                    anim.frame_count = c.u32()?;
                    anim.node_count = c.u32()?;
                    anim.fps = c.f32()?;
                    anim.fps_source = c.f32()?;
                    anim.sample_position_min = c.f32()?;
                    anim.sample_position_scale = c.f32()?;
                    anim.bbox = Aabb::read(c)?;
                    anim.next = text::latin1(c.line(&[]));
                }
                CHUNK_EVENTS => {
                    let count = c.u32()? as usize;
                    anim.events = (0..count).map(|_| read_event(c)).collect::<Result<_>>()?;
                }
                // Relies on the header coming first for the node and frame counts, as it does in
                // every file the engine writes.
                CHUNK_SAMPLES => {
                    anim.checksum = c.u32()?;
                    anim.node_indices =
                        (0..anim.node_count).map(|_| c.u32()).collect::<Result<_>>()?;

                    let count =
                        (anim.node_count as usize).saturating_mul(anim.frame_count as usize);
                    let (scale, min) = (anim.sample_position_scale, anim.sample_position_min);
                    anim.samples = c
                        .records::<[U16; 6]>(count)?
                        .iter()
                        .map(|raw| decode_sample(raw.map(|v| v.get()), scale, min))
                        .collect();
                }
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }

        Ok(anim)
    }

    /// The samples of one frame, one per entry of [`node_indices`](Self::node_indices).
    pub fn frame(&self, frame: usize) -> Option<&[AnimationSample]> {
        let n = self.node_count as usize;
        self.samples.get(frame * n..(frame + 1) * n)
    }
}

fn read_event(c: &mut Reader) -> Result<AnimationEvent> {
    let kind = AnimationEventType::from_raw(c.u32()?);
    let frame = c.u32()?;
    let tag = text::latin1(c.line(WHITESPACE));
    let content = std::array::from_fn(|_| text::latin1(c.line(WHITESPACE)));
    let mut values = [0.0; 4];
    for value in &mut values {
        *value = c.f32()?;
    }
    Ok(AnimationEvent { kind, frame, tag, content, values, probability: c.f32()? })
}

/// Decodes `[rot x, rot y, rot z, pos x, pos y, pos z]` exactly the way ZenKit does.
fn decode_sample(raw: [u16; 6], scale: f32, min: f32) -> AnimationSample {
    let r = |v: u16| (f32::from(v) - ROTATION_MID) * ROTATION_SCALE;
    let (mut x, mut y, mut z) = (r(raw[0]), r(raw[1]), r(raw[2]));

    // The stored vector should be at most unit length so that `w` can be recovered. When rounding
    // pushed it over, it is normalised and `w` becomes zero, as in the engine.
    let len_sq = x * x + y * y + z * z;
    let w = if len_sq > 1.0 {
        let inv = 1.0 / len_sq.sqrt();
        x *= inv;
        y *= inv;
        z *= inv;
        0.0
    } else {
        (1.0 - len_sq).sqrt()
    };

    let p = |v: u16| f32::from(v) * scale + min;
    AnimationSample {
        rotation: Quat::from_xyzw(x, y, z, w),
        position: Vec3::new(p(raw[3]), p(raw[4]), p(raw[5])),
    }
}

impl Asset for ModelAnimation {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        ModelAnimation::from_bytes(data)
    }
}
