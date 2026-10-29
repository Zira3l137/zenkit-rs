//! Worlds (`.ZEN`): the world mesh, its BSP tree, the way-net and every VOB placed in the world.
//!
//! A world is an archive holding one `oCWorld` object, split into sections:
//!
//! - `MeshAndBsp`: the mesh and the BSP tree, written as plain bytes past the archive's encoding.
//!   Only compiled worlds have it; worlds saved by the Spacer without compiling do not.
//! - `VobTree`: the [VOBs](vob), as a tree.
//! - `WayNet`: the points and ways NPCs walk along.
//!
//! Save games store each world the player visited in the same format, with more state for most
//! VOBs, the NPCs, and the weather; see [`WorldState`].
//!
//! ```no_run
//! # fn main() -> zenkit_rs_core::Result<()> {
//! use zenkit_rs_world::World;
//!
//! let world = World::from_bytes(&std::fs::read("NEWWORLD.ZEN").unwrap())?;
//! for (_, vob) in world.walk() {
//!     if let Some(visual) = &vob.visual {
//!         println!("{} at {} shows {}", vob.name, vob.position, visual.name);
//!     }
//! }
//! # Ok(()) }
//! ```

mod bsp;
mod par;
mod save;
pub mod vob;
mod way_net;

use tracing::debug;
use tracing::warn;
use zenkit_rs_archive::Archive;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::text;
use zenkit_rs_mesh::Mesh;
use zenkit_rs_mesh::Triangle;

pub use bsp::BspMode;
pub use bsp::BspNode;
pub use bsp::BspSector;
pub use bsp::BspTree;
pub use save::CutscenePlayer;
pub use save::SkyController;
pub use save::Spawn;
pub use save::WorldState;
pub use vob::Vob;
pub use vob::VobId;
pub use vob::VobKind;
pub use vob::Vobs;
pub use way_net::WayNet;
pub use way_net::WayPoint;

use crate::vob::Ctx;
use crate::vob::FORMAT;

const CHUNK_MESH_END: u16 = 0xB060;

#[derive(Debug, Clone, PartialEq)]
pub struct World {
    pub game: GameVersion,
    /// Empty for worlds that were never compiled.
    pub mesh: Mesh,
    /// Empty for worlds that were never compiled.
    pub bsp: BspTree,
    pub vobs: Vobs,
    /// The VOBs at the top of the VOB tree; the others hang below them as
    /// [`children`](Vob::children).
    pub roots: Vec<VobId>,
    pub way_net: WayNet,
    /// Save games only.
    pub state: Option<WorldState>,
}

impl World {
    /// Reads a world, telling the game it was made for from the data.
    ///
    /// The BSP tree says which game a compiled world is for. Without one, the version of the first
    /// VOB's class tells, if that is a plain VOB or one of its close relatives, as in nearly all
    /// worlds, or the player, as in save games. Otherwise this fails, and
    /// [`from_bytes_as`](Self::from_bytes_as) has to be told the game.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Self::read(data, None)
    }

    /// Reads a world made for `game`.
    pub fn from_bytes_as(data: &[u8], game: GameVersion) -> Result<Self> {
        Self::read(data, Some(game))
    }

    /// Walks the VOB tree depth first, each VOB before its children.
    pub fn walk(&self) -> vob::Walk<'_> {
        self.vobs.walk(&self.roots)
    }

    /// The world mesh's triangles that the game draws.
    ///
    /// As in ZenKit, those are the triangles of the polygons the BSP tree's leaves hold, leaving
    /// out portals, ghost occluders and polygons flagged
    /// [`is_outdoor`](zenkit_rs_mesh::PolygonFlags::is_outdoor). Without a BSP tree, every
    /// polygon counts.
    pub fn triangles(&self) -> impl Iterator<Item = Triangle> + '_ {
        let leaves = &self.bsp.leaf_polygons;
        let all = leaves.is_empty().then_some(0..self.mesh.polygons.len()).into_iter().flatten();
        let listed = leaves.iter().map(|&i| i as usize);

        all.chain(listed)
            .filter(|&i| {
                self.mesh.polygons.get(i).is_some_and(|p| {
                    !(p.flags.is_portal() || p.flags.is_ghost_occluder || p.flags.is_outdoor)
                })
            })
            .flat_map(|i| self.mesh.polygon_triangles(i))
    }

    fn read(data: &[u8], game: Option<GameVersion>) -> Result<Self> {
        let mut archive = Archive::new(data)?;

        let scan = scan(data).context(|| "scanning the sections")?;
        let game = game.or(scan.game).ok_or_else(|| {
            let reason = "cannot tell which game it is for; World::from_bytes_as reads it for one";
            Error::unsupported(FORMAT, reason)
        })?;
        let wide_indices = archive.header().user.as_deref() == Some("XZEN");

        // The mesh and BSP tree are the bulk of a compiled world and sit apart from everything
        // else, so they are read alongside the VOBs.
        let (geometry, sections) = par::join(
            || scan.geometry.map(|g| g.read(wide_indices)).transpose(),
            || read_sections(&mut archive, game),
        );
        let sections = sections?;
        let geometry = match geometry? {
            Some(geometry) => Some(geometry),
            None => sections.geometry.map(|g| g.read(wide_indices)).transpose()?,
        };
        let (mesh, bsp) = geometry.unwrap_or_default();

        Ok(Self {
            game,
            mesh,
            bsp,
            vobs: sections.vobs,
            roots: sections.roots,
            way_net: sections.way_net,
            state: sections.state,
        })
    }
}

impl Asset for World {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        World::from_bytes(data)
    }
}

/// The `MeshAndBsp` section, split into its parts but not read yet.
struct Geometry<'a> {
    version: u32,
    mesh: Reader<'a>,
    bsp: Reader<'a>,
}

impl<'a> Geometry<'a> {
    /// Finds where the mesh and the BSP tree end by walking their chunk headers, and leaves `r`
    /// after both. The section's size field is not trusted, as in ZenKit.
    fn split(r: &mut Reader<'a>) -> Result<Self> {
        let version = r.u32()?;
        let size = r.u32()? as usize;
        let start = r.position();
        let mesh = chunks_until(r, CHUNK_MESH_END).context(|| "mesh")?;
        let bsp = chunks_until(r, bsp::CHUNK_END).context(|| "BSP tree")?;
        if r.position() - start != size {
            debug!("the mesh and BSP tree take {} bytes, not {size}", r.position() - start);
        }
        Ok(Self { version, mesh, bsp })
    }

    fn game(&self) -> GameVersion {
        if self.version == bsp::VERSION_G2 { GameVersion::Gothic2 } else { GameVersion::Gothic1 }
    }

    fn read(mut self, wide_indices: bool) -> Result<(Mesh, BspTree)> {
        if self.version != bsp::VERSION_G1 && self.version != bsp::VERSION_G2 {
            warn!("unknown BSP tree version {:#x}, reading it as Gothic 1", self.version);
        }
        let (mesh, bsp) = par::join(
            || Mesh::read(&mut self.mesh, wide_indices).context(|| "mesh"),
            || BspTree::read(&mut self.bsp, self.version).context(|| "BSP tree"),
        );
        Ok((mesh?, bsp?))
    }
}

/// Splits off everything up to and including the first chunk with id `end`.
fn chunks_until<'a>(r: &mut Reader<'a>, end: u16) -> Result<Reader<'a>> {
    let mut probe = r.remainder();
    while probe.chunk()?.id != end {}
    r.sub(probe.position())
}

/// What a quick look at a world tells before reading it.
struct Scan<'a> {
    game: Option<GameVersion>,
    geometry: Option<Geometry<'a>>,
}

/// Looks at the world's sections until it knows which game the world is for. Compiled worlds start
/// with their mesh and BSP tree, which tell, and are found on the way.
fn scan(data: &[u8]) -> Result<Scan<'_>> {
    let mut archive = Archive::new(data)?;
    archive.expect_object(b"oCWorld:zCWorld")?;

    while !archive.end_object()? {
        let Some(section) = archive.begin_object()? else {
            break;
        };
        match section.name {
            b"MeshAndBsp" => {
                let geometry = archive.read_stream(Geometry::split)?;
                return Ok(Scan { game: Some(geometry.game()), geometry: Some(geometry) });
            }
            b"VobTree" => {
                let mut game = None;
                if archive.int()? > 0
                    && let Some(vob) = archive.begin_object()?
                {
                    game = game_of_class(vob.class, vob.version);
                }
                return Ok(Scan { game, geometry: None });
            }
            _ => archive.skip_object()?,
        }
    }
    Ok(Scan { game: None, geometry: None })
}

/// The class versions that differ between the games. Plain VOBs and a few classes that add
/// nothing to them share `zCVob`'s; save games start with the player, an NPC.
fn game_of_class(class: &[u8], version: u16) -> Option<GameVersion> {
    match (class, version) {
        (b"oCNpc:zCVob", 52224) => Some(GameVersion::Gothic1),
        (b"oCNpc:zCVob", 29697) => Some(GameVersion::Gothic2),
        (b"oCNpc:zCVob", _) => None,
        (_, 12289) if is_plain(class) => Some(GameVersion::Gothic1),
        (_, 52224) if is_plain(class) => Some(GameVersion::Gothic2),
        _ => None,
    }
}

fn is_plain(class: &[u8]) -> bool {
    [&b"zCVob"[..], b"zCVobLevelCompo:zCVob", b"zCVobSpot:zCVob", b"zCVobStartpoint:zCVob"]
        .contains(&class)
}

/// Everything in a world besides its mesh and BSP tree.
struct Sections<'a> {
    /// Only set when the scan did not find the mesh and BSP tree.
    geometry: Option<Geometry<'a>>,
    vobs: Vobs,
    roots: Vec<VobId>,
    way_net: WayNet,
    state: Option<WorldState>,
}

fn read_sections<'a>(archive: &mut Archive<'a>, game: GameVersion) -> Result<Sections<'a>> {
    let world = archive.expect_object(b"oCWorld:zCWorld")?;
    let mut r = Ctx::new(archive, game);
    let (mut geometry, mut roots, mut way_net) = (None, Vec::new(), WayNet::default());
    let (mut cutscene_player, mut sky) = (None, None);

    let mut closed = false;
    loop {
        if r.end_object()? {
            closed = true;
            break;
        }
        let offset = r.offset();
        let Some(section) = r.begin_object()? else {
            return Err(Error::malformed(FORMAT, format!("expected a section at offset {offset}")));
        };

        match section.name {
            b"MeshAndBsp" => geometry = Some(r.read_stream(Geometry::split)?),
            b"VobTree" => {
                let count = r.int()?;
                roots = r.tree(count).context(|| "VOB tree")?;
            }
            b"WayNet" => way_net = WayNet::read(&mut r).context(|| "way-net")?,
            b"CutscenePlayer" if r.save => {
                cutscene_player = CutscenePlayer::read(&mut r).context(|| "cutscene player")?;
            }
            b"SkyCtrl" if r.save => {
                sky = SkyController::read(&mut r).context(|| "sky controller")?;
            }
            b"EndMarker" => {
                r.end(&section)?;
                break;
            }
            name => {
                debug!("skipping world section {:?}", text::latin1(name));
                r.skip_object()?;
                continue;
            }
        }
        r.end(&section)?;
    }
    let mut state = None;
    if r.save {
        let mut save = WorldState { cutscene_player, sky, ..WorldState::default() };
        // Save games store their NPCs inside the world object, after its last section.
        if !closed {
            save.read_npcs(&mut r).context(|| "NPCs")?;
        }
        state = Some(save);
    }
    if !closed {
        r.end(&world)?;
    }

    Ok(Sections { geometry, vobs: r.finish(), roots, way_net, state })
}
