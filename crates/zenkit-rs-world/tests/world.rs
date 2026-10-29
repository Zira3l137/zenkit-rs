use std::path::PathBuf;

use zenkit_rs_core::Error;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_world::BspMode;
use zenkit_rs_world::World;
use zenkit_rs_world::vob::AnimationMode;
use zenkit_rs_world::vob::VisualKind;
use zenkit_rs_world::vob::VobKind;

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn chunk(out: &mut Vec<u8>, id: u16, payload: &[u8]) {
    out.extend(id.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(payload);
}

/// A Gothic 2 mesh with a quad, a portal triangle and a triangle no BSP leaf holds, followed by a
/// BSP tree of a root and two leaves: the `MeshAndBsp` section's contents.
fn geometry() -> Vec<u8> {
    let mut mesh = Vec::new();
    let mut header = 265u16.to_le_bytes().to_vec();
    header.extend([0; 16]);
    header.extend(b"WORLD\n");
    chunk(&mut mesh, 0xB000, &header);

    let mut vertices = 5u32.to_le_bytes().to_vec();
    vertices.extend(floats(&[0., 0., 0., 1., 0., 0., 1., 0., 1., 0., 0., 1., 9., 9., 9.]));
    chunk(&mut mesh, 0xB030, &vertices);

    let mut polygons = 3u32.to_le_bytes().to_vec();
    for (portal, corners) in [(0u8, &[0u32, 1, 2, 3][..]), (1, &[0, 1, 4]), (0, &[1, 2, 4])] {
        polygons.extend(7u16.to_le_bytes());
        polygons.extend((-1i16).to_le_bytes());
        polygons.extend(floats(&[0.0, 0.0, 1.0, 0.0]));
        polygons.push(portal);
        polygons.extend((-1i16).to_le_bytes());
        polygons.push(corners.len() as u8);
        for &corner in corners {
            polygons.extend(corner.to_le_bytes());
            polygons.extend(0u32.to_le_bytes());
        }
    }
    chunk(&mut mesh, 0xB050, &polygons);
    chunk(&mut mesh, 0xB060, &[]);

    let mut header = 2u16.to_le_bytes().to_vec();
    header.extend(1u32.to_le_bytes());
    chunk(&mut mesh, 0xC000, &header);

    let mut indices = 3u32.to_le_bytes().to_vec();
    for i in [0u32, 1, 0] {
        indices.extend(i.to_le_bytes());
    }
    chunk(&mut mesh, 0xC010, &indices);

    let mut tree = 3u32.to_le_bytes().to_vec();
    tree.extend(2u32.to_le_bytes());
    let node = |out: &mut Vec<u8>, offset: u32, count: u32| {
        out.extend(floats(&[-1.0, -1.0, -1.0, 1.0, 1.0, 1.0]));
        out.extend(offset.to_le_bytes());
        out.extend(count.to_le_bytes());
    };
    node(&mut tree, 0, 3);
    tree.push(0b1111);
    tree.extend(floats(&[42.0, 1.0, 0.0, 0.0]));
    node(&mut tree, 0, 2);
    node(&mut tree, 2, 1);
    chunk(&mut mesh, 0xC040, &tree);

    chunk(&mut mesh, 0xC045, &floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]));

    let mut outdoors = 1u32.to_le_bytes().to_vec();
    outdoors.extend(b"CAVE\0");
    for v in [1u32, 1, 1, 2, 1, 1] {
        outdoors.extend(v.to_le_bytes());
    }
    chunk(&mut mesh, 0xC050, &outdoors);
    chunk(&mut mesh, 0xC0FF, b"B");

    let mut section = 0x0409_0000u32.to_le_bytes().to_vec();
    section.extend((mesh.len() as u32).to_le_bytes());
    section.extend(mesh);
    section
}

/// Writes binary archives, the encoding of the worlds the games ship with.
#[derive(Default)]
struct Binary {
    out: Vec<u8>,
    open: Vec<usize>,
    objects: u32,
}

impl Binary {
    /// Opens an object and gives its index, for references to it.
    fn begin(&mut self, name: &str, class: &str, version: u16) -> u32 {
        let index = self.objects;
        self.objects += 1;
        self.object(name, class.as_bytes(), version, index);
        index
    }

    fn object(&mut self, name: &str, class: &[u8], version: u16, index: u32) -> &mut Self {
        self.open.push(self.out.len());
        self.out.extend([0; 4]);
        self.out.extend(version.to_le_bytes());
        self.out.extend(index.to_le_bytes());
        self.out.extend(name.as_bytes());
        self.out.push(0);
        self.out.extend(class);
        self.out.push(0);
        self
    }

    fn end(&mut self) -> &mut Self {
        let start = self.open.pop().unwrap();
        let size = (self.out.len() - start) as u32;
        self.out[start..start + 4].copy_from_slice(&size.to_le_bytes());
        self
    }

    fn reference(&mut self, name: &str, index: u32) -> &mut Self {
        self.object(name, b"\xA7", 0, index).end()
    }

    fn null(&mut self, name: &str) -> &mut Self {
        self.object(name, b"%", 0, 0).end()
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.out.extend(bytes);
        self
    }

    fn int(&mut self, v: i32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    fn string(&mut self, v: &str) -> &mut Self {
        self.bytes(v.as_bytes()).bytes(&[0])
    }

    fn bool(&mut self, v: bool) -> &mut Self {
        self.bytes(&[v as u8])
    }

    /// The start of a packed Gothic 2 VOB, up to its visual object.
    fn vob(&mut self, name: &str, visual: &str, flags: u16, position: [f32; 3]) -> &mut Self {
        let mut raw = floats(&[-1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
        raw.extend(floats(&position));
        raw.extend(floats(&[1., 0., 0., 0., 1., 0., 0., 0., 1.]));
        raw.push(0b0001_1001);
        raw.extend((flags | 0b110).to_le_bytes());
        raw.extend(floats(&[0.0, 1.5]));
        self.int(1).bytes(&raw).string(name).string(visual)
    }

    fn finish(&self, user: &str) -> Vec<u8> {
        let mut out = format!(
            "ZenGin Archive\nver 1\nzCArchiverGeneric\nBINARY\nsaveGame 0\nuser {user}\nEND\n\
             objects {}    \nEND\n",
            self.objects
        )
        .into_bytes();
        out.extend(&self.out);
        out
    }
}

/// A compiled world as the games ship them: mesh and BSP tree, a VOB tree with a shared visual,
/// a door and a VOB of a class nobody knows, and a way-net whose ways refer back to points.
fn compiled_world() -> Vec<u8> {
    let mut b = Binary::default();
    b.begin("%", "oCWorld:zCWorld", 64513);

    b.begin("MeshAndBsp", "%", 0);
    b.bytes(&geometry()).end();

    b.begin("VobTree", "%", 0);
    b.int(2);
    {
        b.begin("%", "zCVobLevelCompo:zCVob", 52224);
        b.vob("LEVEL-VOB", "SURFACE.3DS", 1 << 3, [0.0; 3]);
        b.begin("visual", "zCMesh", 0);
        b.end().end().int(2);

        b.begin("%", "zCVob", 52224);
        b.vob("TREE", "TREE.3DS", 1 << 3 | 2 << 8 | 3 << 10, [1.0, 2.0, 3.0]);
        let shared = b.begin("visual", "zCProgMeshProto", 0);
        b.end().end().int(0);

        b.begin("%", "zCVob", 52224);
        b.vob("TREE2", "TREE.3DS", 1 << 3, [4.0, 5.0, 6.0]);
        b.reference("visual", shared).end().int(1);

        b.begin("%", "oCMobDoor:oCMobInter:oCMOB:zCVob", 64513);
        b.vob("DOOR", "", 1 << 4, [7.0, 8.0, 9.0]).null("ai");
        b.string("MOBNAME_DOOR").int(10).int(0).bool(false).bool(false).bool(false).bytes(&[2]);
        b.string("").string("").string("").bool(false);
        b.int(1).string("").string("").string("").string("").bool(false);
        b.bool(true).string("ITKE_KEY").string("LRL");
        b.end().int(0);

        b.begin("%", "zCVobMagic:zCVob", 0);
        b.int(0).bytes(b"anything");
        b.end().int(1);
        b.begin("%", "zCVob", 52224);
        b.vob("LOST", "", 0, [0.0; 3]);
        b.end().int(0);
    }
    b.end();

    b.begin("WayNet", "%", 0);
    {
        b.begin("%", "zCWayNet", 0);
        b.int(1).int(1);
        let waypoint = |b: &mut Binary, name: &str, x: f32| {
            let index = b.begin("waypoint", "zCWaypoint", 0);
            b.string(name).int(0).bool(false).bytes(&floats(&[x, 0.0, 0.0, 0.0, 0.0, 1.0]));
            b.end();
            index
        };
        let free = waypoint(&mut b, "FREE", 0.0);
        b.int(2);
        let a = waypoint(&mut b, "A", 1.0);
        waypoint(&mut b, "B", 2.0);
        b.reference("wayl1", a).reference("wayr1", free);
        b.end();
    }
    b.end();

    b.begin("EndMarker", "%", 0);
    b.end();
    b.end().finish("luis")
}

#[test]
fn compiled_world_in_binary() {
    let world = World::from_bytes(&compiled_world()).unwrap();
    assert_eq!(world.game, GameVersion::Gothic2);

    assert_eq!(world.mesh.name, "WORLD");
    assert_eq!((world.mesh.vertices.len(), world.mesh.polygons.len()), (5, 3));

    let bsp = &world.bsp;
    assert_eq!(bsp.mode, BspMode::Outdoor);
    assert_eq!(bsp.polygon_indices, [0, 1, 0]);
    assert_eq!(bsp.nodes.len(), 3);
    let root = &bsp.nodes[0];
    assert_eq!((root.front, root.back, root.parent), (Some(1), Some(2), None));
    assert_eq!((root.plane_normal, root.plane_distance), (Vec3::X, 42.0));
    assert!(!root.is_leaf());
    assert!(bsp.nodes[1].is_leaf() && bsp.nodes[2].is_leaf());
    assert_eq!(bsp.nodes[2].parent, Some(0));
    assert_eq!(bsp.node_polygons(&bsp.nodes[1]), [0, 1]);
    assert_eq!(bsp.leaf_nodes, [1, 2]);
    assert_eq!(bsp.leaf_polygons, [0, 1]);
    assert_eq!(bsp.light_points, [Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)]);
    assert_eq!(bsp.sectors.len(), 1);
    assert_eq!(bsp.sectors[0].name, "CAVE");
    assert_eq!(bsp.sectors[0].node_indices, [1]);
    assert_eq!(bsp.sectors[0].portal_polygon_indices, [2]);
    assert_eq!(bsp.portal_polygon_indices, [1]);

    // Only the quad: the portal is left out and no leaf holds the last triangle.
    let triangles: Vec<_> = world.triangles().map(|t| (t.polygon, t.vertices)).collect();
    assert_eq!(triangles, [(0, [0, 1, 2]), (0, [0, 2, 3])]);

    assert_eq!(world.roots.len(), 1);
    let names: Vec<_> = world.walk().map(|(_, vob)| vob.name.as_str()).collect();
    assert_eq!(names, ["LEVEL-VOB", "TREE", "TREE2", "DOOR"]);
    assert_eq!(world.vobs.len(), 4, "the unknown VOB and its child are skipped");

    let level = &world.vobs[world.roots[0]];
    assert_eq!(level.kind, VobKind::Level);
    assert_eq!(level.visual.as_ref().unwrap().kind, VisualKind::Mesh);
    let [tree, tree2] = level.children[..] else { panic!("{:?}", level.children) };

    let tree = &world.vobs[tree];
    assert_eq!(tree.position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!((tree.anim_mode, tree.bias, tree.far_clip_scale), (AnimationMode::WindAlt, 3, 1.5));
    assert!(tree.show_visual && tree.cd_static && tree.cd_dynamic && !tree.is_static);

    let tree2 = &world.vobs[tree2];
    let visual = tree2.visual.as_ref().unwrap();
    assert_eq!(
        (visual.name.as_str(), &visual.kind),
        ("TREE.3DS", &VisualKind::MultiResolutionMesh)
    );

    let door = &world.vobs[tree2.children[0]];
    assert_eq!(door.visual, None);
    let VobKind::Door(d) = &door.kind else { panic!("{:?}", door.kind) };
    assert!(d.locked);
    assert_eq!((d.key.as_str(), d.pick_string.as_str()), ("ITKE_KEY", "LRL"));
    assert_eq!(door.kind.mob().unwrap().focus_name, "MOBNAME_DOOR");

    let net = &world.way_net;
    let names: Vec<_> = net.points.iter().map(|p| (p.name.as_str(), p.free_point)).collect();
    assert_eq!(names, [("FREE", true), ("A", false), ("B", false)]);
    assert_eq!(net.ways, [(1, 2), (1, 0)]);
    assert_eq!(net.point("b").map(|(i, p)| (i, p.position.x)), Some((2, 2.0)));
}

#[test]
fn broken_worlds_fail_cleanly() {
    let data = compiled_world();
    for len in 0..data.len() {
        let _ = World::from_bytes(&data[..len]);
    }
}

/// An uncompiled Gothic 1 world: no mesh, so only the first VOB's class version tells the game.
/// `mesh` adds a mesh and BSP tree after the VOB tree, which the Spacer never does, but the
/// engine would read.
fn ascii_world(vob_version: u16, mesh: bool) -> Vec<u8> {
    let mut out = format!(
        "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\nobjects 4\nEND\n\n\
         [% oCWorld:zCWorld 64513 0]\n\t[VobTree % 0 0]\n\t\tchilds0=int:1\n\
         \t\t[% zCVob {vob_version} 1]\n\t\t\tpack=int:0\n\t\t\tpresetName=string:\n\
         \t\t\tbbox3DWS=rawFloat:0 0 0 1 1 1\n\
         \t\t\ttrafoOSToWSRot=raw:0000803f0000000000000000000000000000803f000000000000000000000000000000000000803f\n\
         \t\t\ttrafoOSToWSPos=vec3:5 6 7\n\t\t\tvobName=string:STONE\n\t\t\tvisual=string:STONE.3DS\n\
         \t\t\tshowVisual=bool:1\n\t\t\tvisualCamAlign=enum:0\n\t\t\tcdStatic=bool:1\n\
         \t\t\tcdDyn=bool:1\n\t\t\tstaticVob=bool:1\n\t\t\tdynShadow=enum:0\n\
         \t\t\t[visual zCProgMeshProto 0 2]\n\t\t\t[]\n\t\t\t[ai % 0 0]\n\t\t\t[]\n\t\t[]\n\
         \t\tchilds1=int:0\n\t[]\n"
    )
    .into_bytes();
    if mesh {
        out.extend(b"\t[MeshAndBsp % 0 0]\n");
        out.extend(geometry());
        out.extend(b"\t[]\n");
    }
    out.extend(b"\t[EndMarker % 0 0]\n\t[]\n[]\n");
    out
}

#[test]
fn the_first_vob_tells_the_game() {
    let world = World::from_bytes(&ascii_world(12289, false)).unwrap();
    assert_eq!(world.game, GameVersion::Gothic1);
    assert!(world.mesh.polygons.is_empty() && world.bsp.nodes.is_empty());
    assert_eq!(world.triangles().count(), 0);

    let stone = &world.vobs[world.roots[0]];
    assert_eq!(stone.name, "STONE");
    assert_eq!(stone.position, Vec3::new(5.0, 6.0, 7.0));
    assert!(stone.is_static && stone.physics_enabled);
    assert_eq!(stone.far_clip_scale, 2.0);

    let unknown = ascii_world(0, false);
    let err = World::from_bytes(&unknown).unwrap_err();
    assert!(matches!(err.root(), Error::Unsupported { .. }), "{}", err.report());
    let world = World::from_bytes_as(&unknown, GameVersion::Gothic1).unwrap();
    assert_eq!(world.vobs.len(), 1);
}

#[test]
fn a_mesh_after_the_vobs_is_read_too() {
    let world = World::from_bytes(&ascii_world(12289, true)).unwrap();
    assert_eq!(world.game, GameVersion::Gothic1);
    assert_eq!(world.mesh.polygons.len(), 3);
    assert_eq!(world.bsp.leaf_polygons, [0, 1]);
    assert_eq!(world.vobs.len(), 1);
}

/// Gothic 1's world, if `ZENKIT_SAMPLES` points at a ZenKit checkout's `tests/samples` with a copy
/// of `WORLD.ZEN` named `world.proprietary.zen` in it, as ZenKit's own tests want it. The checks are
/// ZenKit's.
#[test]
fn zenkit_gothic1_world() {
    let path =
        std::env::var_os("ZENKIT_SAMPLES").map(|d| PathBuf::from(d).join("world.proprietary.zen"));
    let Some(data) = path.and_then(|path| std::fs::read(path).ok()) else {
        eprintln!("no world.proprietary.zen in ZENKIT_SAMPLES, skipping");
        return;
    };
    let world = World::from_bytes(&data).unwrap();
    assert_eq!(world.game, GameVersion::Gothic1);

    let mesh = &world.mesh;
    assert_eq!(
        (mesh.vertices.len(), mesh.features.len(), mesh.materials.len()),
        (55439, 419936, 2263)
    );
    assert_eq!(mesh.vertices[0], Vec3::new(91365.0, -4026.6008, 46900.0));
    assert_eq!(mesh.vertices[500], Vec3::new(44263.82, 708.5178, 6841.1826));
    assert_eq!(mesh.features[0].light, 4292927712);
    assert_eq!(mesh.materials[0].name, "OWODWATSTOP");
    assert_eq!(mesh.materials[500].texture, "OMWABROWNGREEN01.TGA");

    let triangles: Vec<_> = world.triangles().collect();
    assert_eq!(triangles.len(), 106722);
    let polygon = |i: usize| &mesh.polygons[triangles[i].polygon];
    assert_eq!(triangles[0].vertices, [0, 2, 1]);
    assert_eq!(triangles[0].features, [0, 1, 2]);
    assert_eq!(triangles[26680].vertices, [14241, 14243, 14242]);
    assert_eq!(triangles[53360].vertices, [28518, 28512, 28520]);
    assert_eq!(triangles[53360].features, [210349, 210350, 210351]);
    assert_eq!(triangles[106721].vertices, [55429, 55428, 54576]);
    assert_eq!(triangles[106721].features, [419933, 419934, 419935]);
    assert_eq!((polygon(0).material, polygon(0).lightmap), (0, None));
    assert_eq!((polygon(53360).material, polygon(53360).lightmap), (1097, Some(557)));
    assert!(polygon(53360).flags.is_occluder && polygon(53360).flags.is_sector);

    let bsp = &world.bsp;
    assert_eq!(bsp.mode, BspMode::Outdoor);
    assert_eq!(bsp.polygon_indices.len(), 480135);
    assert_eq!(bsp.polygon_indices[150..153], [102, 103, 92]);
    assert_eq!(bsp.nodes.len(), 6644);
    let root = &bsp.nodes[0];
    assert_eq!((root.plane_normal, root.plane_distance), (Vec3::X, 18540.016));
    assert_eq!((root.front, root.back, root.parent), (Some(1), Some(1599), None));
    assert_eq!(root.bbox.min, Vec3::new(-71919.96, -12000.0, -59900.0));
    assert_eq!(bsp.nodes[1].plane_normal, Vec3::Z);
    assert_eq!((bsp.nodes[1].front, bsp.nodes[1].back), (Some(2), Some(445)));
    assert_eq!(bsp.leaf_nodes.len(), 3318);
    assert_eq!((bsp.leaf_nodes[0], bsp.leaf_nodes[10]), (5, 26));
    let leaf = &bsp.nodes[26];
    assert!(leaf.is_leaf());
    assert_eq!((leaf.parent, leaf.polygon_offset, leaf.polygon_count), (Some(25), 446, 24));
    assert_eq!(bsp.sectors.len(), 299);
    assert_eq!(bsp.sectors[50].name, "OWCAVE01");
    assert_eq!(bsp.sectors[50].node_indices.len(), 4);
    assert!(bsp.portal_polygon_indices.is_empty());
    assert_eq!(bsp.light_points.len(), 3318);
    assert_eq!(bsp.light_points[0], Vec3::splat(-99.0));

    assert_eq!(world.roots.len(), 14);
    let first = &world.vobs[world.roots[0]];
    assert_eq!(first.name, "LEVEL-VOB");
    assert_eq!(first.visual.as_ref().unwrap().name, "SURFACE.3DS");
    assert!(first.cd_static && !first.cd_dynamic && !first.physics_enabled);
    assert_eq!(first.children.len(), 7496);
    let campfire = &world.vobs[first.children[0]];
    assert_eq!(campfire.name, "FP_CAMPFIRE_PATH_BANDITOS2_03_02");
    assert_eq!(campfire.position, Vec3::new(-18544.486, -136.1719, 4141.1973));
    assert_eq!(campfire.rotation.x_axis, Vec3::new(-0.779_197, 0.0, -0.626_779_1));
    assert!(campfire.children.is_empty());
    let last = &world.vobs[world.roots[13]];
    assert_eq!(last.visual.as_ref().unwrap().name, "OLDCAMP.3DS");
    assert_eq!(last.children.len(), 3250);

    let net = &world.way_net;
    assert_eq!((net.points.len(), net.ways.len()), (2784, 3500));
    assert_eq!(net.points[0].name, "LOCATION_28_07");
    assert_eq!(net.points[0].position, Vec3::new(23871.457, -553.2838, 27821.352));
    assert_eq!(net.points[100].name, "CASTLE_MOVEMENT_STRAIGHT3");
    assert_eq!(net.points[100].direction, Vec3::new(-0.342_115_37, 0.0, 0.939_657_9));
}
