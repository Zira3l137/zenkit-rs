//! Morph meshes (`.MMB`): meshes animated by moving vertices directly, used for faces and for
//! things like bows bending.

use crate::Asset;
use crate::Date;
use crate::Result;
use crate::io::Reader;
use crate::io::WHITESPACE;
use crate::math::Vec3;
use crate::mrm::MultiResolutionMesh;
use crate::text;

const FORMAT: &str = "morph mesh";

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
                id => log::debug!("{FORMAT}: skipping unknown chunk {id:#06x}"),
            }
        }

        Ok(morph)
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
