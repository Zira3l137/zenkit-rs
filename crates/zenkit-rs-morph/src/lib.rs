//! Morph meshes (`.MMB`): meshes animated by moving vertices directly, used for faces and for
//! things like bows bending.
//!
//! ```text
//! E000 marker (empty) | E020 header: version, name | B100 multi-resolution mesh
//! B1FF morph positions | E010 sources | E030 animations
//! ```
//!
//! [`MorphMesh::to_bytes`] writes a morph mesh back in the same order.

use tracing::debug;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Date;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::io::WHITESPACE;
use zenkit_rs_core::io::Writer;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;
use zenkit_rs_mrm::MultiResolutionMesh;

const FORMAT: &str = "morph mesh";

/// The version the game's morph meshes carry, Gothic 1's and Gothic II's alike.
const VERSION: u32 = 3;

const CHUNK_MARKER: u16 = 0xE000;
const CHUNK_SOURCES: u16 = 0xE010;
const CHUNK_HEADER: u16 = 0xE020;
const CHUNK_ANIMATIONS: u16 = 0xE030;
const CHUNK_MRM: u16 = 0xB100;
const CHUNK_MORPH: u16 = 0xB1FF;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MorphAnimation {
    pub name: String,
    pub layer: i32,
    pub blend_in: f32,
    pub blend_out: f32,
    /// In milliseconds.
    pub duration: f32,
    pub speed: f32,
    pub flags: u8,
    pub frame_count: u32,
    /// The indices of the positions this animation moves.
    pub vertices: Vec<u32>,
    /// For each frame, one offset per entry of [`vertices`](Self::vertices).
    pub samples: Vec<Vec3>,
}

impl MorphAnimation {
    /// The samples of one frame, one per entry of [`vertices`](Self::vertices).
    pub fn frame(&self, frame: usize) -> Option<&[Vec3]> {
        let n = self.vertices.len();
        self.samples.get(frame * n..(frame + 1) * n)
    }
}

/// A source file the mesh was compiled from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MorphSource {
    pub date: Date,
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MorphMesh {
    pub name: String,
    pub mesh: MultiResolutionMesh,
    /// The rest positions animations are applied to, one per position of [`mesh`](Self::mesh).
    pub morph_positions: Vec<Vec3>,
    pub animations: Vec<MorphAnimation>,
    pub sources: Vec<MorphSource>,
}

impl MorphMesh {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let mut morph = Self::default();

        // Morph meshes have no end chunk; they simply stop.
        while !r.is_eof() {
            let mut chunk = r.chunk()?;
            let c = &mut chunk.data;
            match chunk.id {
                CHUNK_MARKER => {}
                CHUNK_SOURCES => {
                    let count = c.u16()? as usize;
                    morph.sources = (0..count)
                        .map(|_| {
                            Ok(MorphSource {
                                date: Date::read(c)?,
                                file_name: text::latin1(c.line(WHITESPACE)),
                            })
                        })
                        .collect::<Result<_>>()?;
                }
                CHUNK_HEADER => {
                    let _version = c.u32()?;
                    morph.name = text::latin1(c.line(WHITESPACE));
                }
                CHUNK_MRM => morph.mesh = MultiResolutionMesh::read_section(c)?,
                // Holds one position per MRM position, so it relies on the MRM chunk coming first,
                // as it does in every file the engine writes.
                CHUNK_MORPH => morph.morph_positions = c.vec3s(morph.mesh.positions.len())?,
                CHUNK_ANIMATIONS => {
                    let count = c.u16()? as usize;
                    morph.animations =
                        (0..count).map(|_| read_animation(c)).collect::<Result<_>>()?;
                }
                id => debug!("skipping unknown chunk {id:#06x}"),
            }
        }

        Ok(morph)
    }

    /// Writes the morph mesh as a `.MMB` file, with its mesh made for `game`.
    ///
    /// Fails if there is not one morph position per position of the mesh, an animation does
    /// not have one sample per vertex and frame, there are more than 65535 animations or sources,
    /// a name has a line break, or the mesh cannot be written.
    pub fn to_bytes(&self, game: GameVersion) -> Result<Vec<u8>> {
        let fail = |reason: String| Err(Error::unwritable(FORMAT, reason));
        if self.morph_positions.len() != self.mesh.positions.len() {
            let (morph, mesh) = (self.morph_positions.len(), self.mesh.positions.len());
            return fail(format!("{morph} morph positions for {mesh} mesh positions"));
        }
        for anim in &self.animations {
            let expected = anim.vertices.len() as u64 * u64::from(anim.frame_count);
            if anim.samples.len() as u64 != expected {
                let (count, frames) = (anim.vertices.len(), anim.frame_count);
                return fail(format!(
                    "animation {:?} has {} samples, not one per vertex and frame ({count} x {frames})",
                    anim.name,
                    anim.samples.len()
                ));
            }
        }
        let count = |len: usize, what: &str| {
            u16::try_from(len)
                .map_err(|_| Error::unwritable(FORMAT, format!("{len} {what}, at most 65535 fit")))
        };
        let source_count = count(self.sources.len(), "sources")?;
        let animation_count = count(self.animations.len(), "animations")?;
        let line = |text: &str| text::to_line(FORMAT, text);
        let name = line(&self.name)?;
        let sources: Vec<_> = (self.sources.iter())
            .map(|source| Ok((source.date, line(&source.file_name)?)))
            .collect::<Result<_>>()?;
        let names: Vec<_> =
            self.animations.iter().map(|anim| line(&anim.name)).collect::<Result<_>>()?;

        let mut w = Writer::new();
        w.chunk(CHUNK_MARKER, |_| ());
        w.chunk(CHUNK_HEADER, |w| {
            w.u32(VERSION);
            w.line(&name);
        });
        w.chunk(CHUNK_MRM, |w| self.mesh.write_section(w, game))?;
        w.chunk(CHUNK_MORPH, |w| w.vec3s(&self.morph_positions));
        w.chunk(CHUNK_SOURCES, |w| {
            w.u16(source_count);
            for (date, file_name) in &sources {
                date.write(w);
                w.line(file_name);
            }
        });
        w.chunk(CHUNK_ANIMATIONS, |w| {
            w.u16(animation_count);
            for (anim, name) in self.animations.iter().zip(&names) {
                w.line(name);
                w.f32(anim.blend_in);
                w.f32(anim.blend_out);
                w.f32(anim.duration);
                w.i32(anim.layer);
                w.f32(anim.speed);
                w.u8(anim.flags);
                w.u32(anim.vertices.len() as u32);
                w.u32(anim.frame_count);
                anim.vertices.iter().for_each(|&v| w.u32(v));
                w.vec3s(&anim.samples);
            }
        });
        Ok(w.into_inner())
    }
}

fn read_animation(c: &mut Reader) -> Result<MorphAnimation> {
    let name = text::latin1(c.line(&[]));
    let blend_in = c.f32()?;
    let blend_out = c.f32()?;
    let duration = c.f32()?;
    let layer = c.i32()?;
    let speed = c.f32()?;
    let flags = c.u8()?;
    let vertex_count = c.u32()? as usize;
    let frame_count = c.u32()?;

    let vertices = (0..vertex_count).map(|_| c.u32()).collect::<Result<_>>()?;
    let samples = c.vec3s(vertex_count.saturating_mul(frame_count as usize))?;

    Ok(MorphAnimation {
        name,
        layer,
        blend_in,
        blend_out,
        duration,
        speed,
        flags,
        frame_count,
        vertices,
        samples,
    })
}

impl Asset for MorphMesh {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        MorphMesh::from_bytes(data)
    }
}
