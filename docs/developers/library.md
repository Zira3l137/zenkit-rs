# Using the library

A tour of zenkit-rs for Rust programs: how to get at the game's files, read and write them, and
handle what goes wrong. The API reference is on [docs.rs](https://docs.rs/zenkit-rs); this
explains how the pieces fit. Every example here is compiled and run by the tests, so they keep up
with the API; the ones that need the game's files are only compiled.

The examples call the package `zenkit`, as the README's dependency line renames it:

```toml
[dependencies]
zenkit = { package = "zenkit-rs", version = "0.1" }
```

## The shape of it

Every format is a crate of its own (`zenkit-rs-mrm`, `zenkit-rs-world`…), pulled in by the cargo
feature of the same name and re-exported as a module (`zenkit::mrm`, `zenkit::world`). All
formats are on by default; a program that only needs textures can say
`default-features = false, features = ["texture"]` and compile nothing else. The README lists the
features.

Each format has a type with the same pair of methods: `from_bytes` to read it, and `to_bytes` to
write it. Writing takes the game the file is for where the two games' formats differ, as
[`GameVersion`](https://docs.rs/zenkit-rs/latest/zenkit/enum.GameVersion.html):

| Format                         | Type                                   | Writes with                    |
|--------------------------------|----------------------------------------|--------------------------------|
| `.VDF` archives                | `vfs::Vfs`, `vfs::Node`                | `write_disk(out, &DiskOptions)`|
| Textures `.TEX`                | `texture::Texture`                     | `to_bytes()`                   |
| Static meshes `.MSH`           | `mesh::Mesh`                           | `to_bytes(game)`               |
| Multi-resolution meshes `.MRM` | `mrm::MultiResolutionMesh`             | `to_bytes(game)`               |
| Skeletons `.MDH`               | `model::ModelHierarchy`                | `to_bytes()`                   |
| Model meshes `.MDM`            | `model::ModelMesh`                     | `to_bytes(game)`               |
| Models `.MDL`                  | `model::Model`                         | `to_bytes(game)`               |
| Morph meshes `.MMB`            | `morph::MorphMesh`                     | `to_bytes(game)`               |
| Animations `.MAN`              | `animation::ModelAnimation`            | `to_bytes()`                   |
| Model scripts `.MDS`, `.MSB`   | `model_script::ModelScript`            | `to_mds()`, `to_msb()`         |
| Worlds `.ZEN`                  | `world::World`                         | `to_bytes(format)`             |
| Cutscene libraries `OU.BIN`    | `cutscene::CutsceneLibrary`            | `to_bytes(format)`             |
| Save games                     | `save::SaveGame`                       | `write(folder, worlds)`        |
| Fonts `.FNT`                   | `font::Font`                           | `to_bytes()`                   |

Writers follow the engine where it and ZenKit disagree, so that the game takes what they write;
[engine-quirks.md](engine-quirks.md) collects what that took.

## Getting at the game's files

The game never opens its assets directly. It mounts every `.VDF` archive in `Data/` into one
directory tree and looks files up there by name. A [`Vfs`](https://docs.rs/zenkit-rs/latest/zenkit/vfs/struct.Vfs.html)
does the same:

```rust,no_run
use zenkit::mrm::MultiResolutionMesh;
use zenkit::texture::Texture;
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;

fn main() -> zenkit::Result<()> {
    let mut vfs = Vfs::new();
    // When two archives hold the same file, the newer archive wins, as in the game.
    vfs.mount_disks(&["Data/Meshes.vdf", "Data/Textures.vdf"], Overwrite::Older)?;

    // Names are found anywhere in the tree, ignoring case, as the engine finds them.
    let chest: MultiResolutionMesh = vfs.load("CHESTSMALL_OCCHESTSMALL.MRM")?;
    let wood: Texture = vfs.load("OCODFLWOOD-C.TEX")?;
    println!("{} sub-meshes, texture {} x {}", chest.sub_meshes.len(), wood.width, wood.height);
    Ok(())
}
```

`load` reads any type that implements [`Asset`](https://docs.rs/zenkit-rs/latest/zenkit/trait.Asset.html),
which every format does. `load_all` loads a list of names at once, on every CPU core with the
`parallel` feature. `find` gives a file's [`Node`](https://docs.rs/zenkit-rs/latest/zenkit/vfs/struct.Node.html)
by name, `resolve` by path (`/_WORK/DATA/MESHES/...`), and `files` walks them all.

Assets and their references name source files, which the archives hold compiled under other
names: a material's `STONE.TGA` is `STONE-C.TEX`, a VOB's `CHEST.3DS` is `CHEST.MRM`, a model
script's `HUMANS.MDS` is `HUMANS.MSB`. The `browse` example's `compiled_name` does that mapping.

Loose files go into the same tree with `mount_host`, under a directory of your choosing:

```rust,no_run
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;

fn main() -> zenkit::Result<()> {
    let mut vfs = Vfs::new();
    vfs.mount_disks(&["Data/Anims.vdf"], Overwrite::Older)?;
    vfs.mkdir("/_WORK")?;
    // A mod's loose files on top, replacing the archive's where they share a name.
    vfs.mount_host("MyMod/_work", "/_WORK", Overwrite::All)?;
    Ok(())
}
```

With the `mmap` feature, archives are memory-mapped rather than read into memory, so only what is
looked up is read. Leave it off unless you control the files: if another program changes a mapped
archive while it is mounted, the data being read changes with it.

## When things go wrong

Every crate returns the same [`zenkit::Error`](https://docs.rs/zenkit-rs/latest/zenkit/enum.Error.html).
Its message says only what went wrong; where it went wrong is in its sources, the file, the
object, the chunk, which `report` prints in one line:

```rust,no_run
use zenkit::Error;
use zenkit::model::ModelMesh;
use zenkit::vfs::Vfs;

fn show(vfs: &Vfs) {
    match vfs.load::<ModelMesh>("ARMOR_NEW.MDM") {
        Ok(mesh) => println!("{} skinned meshes", mesh.meshes.len()),
        // `root` looks past the context, to tell what kind of failure it was.
        Err(e) if matches!(e.root(), Error::NotFound(_)) => println!("no such file"),
        // cannot load "ARMOR_NEW.MDM": attachment "ZM_BELT": the data ends early: ...
        Err(e) => eprintln!("{}", e.report()),
    }
}
# fn main() {}
```

Offsets in messages count from the start of the file. Readers fail on data they cannot make
sense of; where they can carry on, like an unknown chunk or a value out of range, they log a
warning instead and use a default.

The crates log through [tracing](https://docs.rs/tracing). Nothing is printed until the program
installs a subscriber, for example with `tracing-subscriber`:

```rust,ignore
tracing_subscriber::fmt().with_env_filter("zenkit=warn").init();
```

Warnings raised while loading through a `Vfs` sit in a span naming the file. Programs that log
through the `log` crate instead turn on the `log` feature.

## Writing files

Writing is the reverse of reading. A texture from RGBA pixels, with the mipmaps the game takes:

```rust
use zenkit::texture::Texture;
use zenkit::texture::TextureFormat;
use zenkit::texture::game_mipmap_count;

fn main() -> zenkit::Result<()> {
    let (width, height) = (64, 64);
    let rgba = vec![200u8; (width * height * 4) as usize];

    // DXT1 or DXT3: the game refuses DXT5, and mipmaps under 4 x 4 pixels.
    let format = TextureFormat::Dxt1;
    let texture = Texture::from_rgba8(width, height, &rgba, format, game_mipmap_count(format, width, height))?;
    let bytes = texture.to_bytes();

    assert_eq!(Texture::from_bytes(&bytes)?.mipmap_count(), 5); // 64, 32, 16, 8, 4
    Ok(())
}
```

A file that cannot be stored fails with `Error::Unwritable`, saying why: a name with a line break,
more vertices than the format counts, a skin that names a bone the skeleton lacks. Things the game
would refuse but the format can hold, like a DXT5 texture, are written with a warning, since
other programs may read them.

### Packing a mod

A mod ships as a `.VDF` in the game's `Data/` folder. Build its tree from nodes, or mount a folder,
and write it:

```rust
use std::time::SystemTime;

use zenkit::GameVersion;
use zenkit::vfs::DiskOptions;
use zenkit::vfs::Node;
use zenkit::vfs::Overwrite;
use zenkit::vfs::Vfs;

fn main() -> zenkit::Result<()> {
    let now = SystemTime::now();
    let mut vfs = Vfs::new();
    let textures = vfs.mkdir("/_WORK/DATA/TEXTURES/_COMPILED")?;
    textures.insert(Node::file("MYSTONE-C.TEX", vec![0u8; 16], now))?;

    let mut archive = Vec::new();
    vfs.write_disk(&mut archive, &DiskOptions::new(GameVersion::Gothic2))?;

    let mut back = Vfs::new();
    back.mount_disk_bytes(archive, Overwrite::Older)?;
    assert!(back.find("mystone-c.tex").is_some());
    Ok(())
}
```

Names are stored upper case, as the game's tools store them. The archive's date is its newest
file's, which makes it win over the game's own archives. `DiskOptions::compressed` writes Union's
compressed archives, which only games running Union can read.

## Worlds and VOBs

A [`World`](https://docs.rs/zenkit-rs/latest/zenkit/world/struct.World.html) is its mesh and BSP
tree, which compiled worlds have, the tree of VOBs (everything placed in it), and the way-net.
VOBs refer to each other and one can sit in several places, so they are kept flat in
[`Vobs`](https://docs.rs/zenkit-rs/latest/zenkit/world/vob/struct.Vobs.html) and referred to by
[`VobId`](https://docs.rs/zenkit-rs/latest/zenkit/world/vob/struct.VobId.html). A VOB's `kind`
holds what its class adds, with accessors like `mob()` that find a part whatever the exact class:

```rust,no_run
use zenkit::vfs::Vfs;
use zenkit::world::World;
use zenkit::world::vob::VobKind;

fn list(vfs: &Vfs) -> zenkit::Result<()> {
    let world: World = vfs.load("NEWWORLD.ZEN")?;
    // Depth first, each VOB before its children.
    for (_, vob) in world.walk() {
        match &vob.kind {
            VobKind::Light(light) => println!("light {:?}, range {}", vob.name, light.range),
            kind if kind.mob().is_some() => println!("mob {:?}", kind.mob().unwrap().focus_name),
            _ => {}
        }
    }
    println!("{} triangles drawn", world.triangles().count());
    Ok(())
}
# fn main() {}
```

Adding a VOB is pushing it into `vobs` and putting its id in the tree: into `roots`, or another
VOB's `children`. Writing picks the archive encoding; the game reads all three, its own worlds are
BIN_SAFE:

```rust
use zenkit::GameVersion;
use zenkit::archive::Format;
use zenkit::world::World;
use zenkit::world::vob::Vob;
use zenkit::world::vob::Vobs;

fn main() -> zenkit::Result<()> {
    let mut world = World {
        game: GameVersion::Gothic2,
        mesh: Default::default(),
        bsp: Default::default(),
        vobs: Vobs::default(),
        roots: Vec::new(),
        way_net: Default::default(),
        state: None,
    };
    let spot = world.vobs.push(Vob { name: "FP_MY_SPOT".into(), ..Vob::default() });
    world.roots.push(spot);

    let bytes = world.to_bytes(Format::Binsafe)?;
    let back = World::from_bytes_as(&bytes, GameVersion::Gothic2)?;
    assert_eq!(back.vobs[back.roots[0]].name, "FP_MY_SPOT");
    Ok(())
}
```

To write a world back as it came, pass the header it was read with to `to_bytes_with`. A header
whose `save_game` is set writes the world as save games store it, with every VOB's state.

## Save games

A save is a folder: what the load menu shows, the scripts' state, the quest log, a thumbnail, and
a world for every level the player visited, which are only read when asked for:

```rust,no_run
use zenkit::save::SaveGame;

fn main() -> zenkit::Result<()> {
    let mut save = SaveGame::open("Saves/savegame1")?;
    println!("{} in {}", save.info.title, save.info.world);
    println!("{:?}", save.data.symbol("KAPITEL")); // a script variable
    for topic in &save.data.log {
        println!("{}: {} entries", topic.description, topic.entries.len());
    }

    let world = save.current_world()?;
    save.info.title = "Before the mine".into();
    // Worlds not given are copied from the save's own folder.
    save.write("Saves/savegame2", &[(save.info.world.as_str(), &world)])?;
    Ok(())
}
```

## Models and animations

A skeleton (`.MDH`, [`ModelHierarchy`](https://docs.rs/zenkit-rs/latest/zenkit/model/struct.ModelHierarchy.html))
is a list of nodes, each with its parent and its transform relative to it. Meshes that bend with
it (`.MDM`, [`ModelMesh`](https://docs.rs/zenkit-rs/latest/zenkit/model/struct.ModelMesh.html))
store the checksum of the skeleton they were made for, and the game refuses them on any other; a
skeleton's checksum is the CRC-32 of its node names, which `node_checksum` computes. A `.MDL` is
both in one file. Animations (`.MAN`) carry the checksum too, and name nodes by their place in the
skeleton:

```rust,no_run
use zenkit::animation::ModelAnimation;
use zenkit::model::ModelHierarchy;
use zenkit::model::ModelMesh;
use zenkit::vfs::Vfs;

fn check(vfs: &Vfs) -> zenkit::Result<()> {
    let humans: ModelHierarchy = vfs.load("HUMANS.MDH")?;
    let body: ModelMesh = vfs.load("HUM_BODY_NAKED0.MDM")?;
    let run: ModelAnimation = vfs.load("HUMANS-S_RUN.MAN")?;
    assert_eq!(body.checksum, humans.checksum);
    assert_eq!(run.checksum, humans.checksum);
    println!("{} nodes, {} frames at {} fps", humans.nodes.len(), run.frame_count, run.fps);
    Ok(())
}
# fn main() {}
```

An animation's positions are packed into 16 bits over the range they span: after changing
samples, call `fit_sample_positions` before writing. Model scripts read from either form,
`ModelScript::from_bytes`, and write to both, `to_mds` and `to_msb`; the animations' names, layers
and events are in `statements`.

## Dialogue and fonts

`OU.BIN` (or `OU.CSL`) holds every line of dialogue, found by the name of its sound file without
the extension, ignoring case:

```rust,no_run
use zenkit::cutscene::CutsceneLibrary;
use zenkit::vfs::Vfs;

fn line(vfs: &Vfs) -> zenkit::Result<()> {
    let library: CutsceneLibrary = vfs.load("OU.BIN")?;
    if let Some(block) = library.block("DIA_ARTO_PERM_15_00") {
        println!("{}", block.message.text);
    }
    Ok(())
}
# fn main() {}
```

The text is decoded as Latin-1, which keeps every byte; a German or Polish game's text may be in
another code page, which the bytes can be turned back into. A font (`.FNT`) says where each
character is in its texture; `Font::glyph('A')` finds a character by its Windows-1252 byte.

## Lower down

The formats are built on two layers anyone can use for formats of their own:

- [`zenkit::archive`](https://docs.rs/zenkit-rs/latest/zenkit/archive/index.html), the ZenGin's
  object format in its three encodings (ASCII, binary, BIN_SAFE): `Archive` reads values in
  order, `ArchiveWriter` writes them, and object indices and references are handled for you.
- [`zenkit::io`](https://docs.rs/zenkit-rs/latest/zenkit/io/index.html), `Reader` and `Writer`
  for the binary formats: little-endian numbers, chunks, lines, and fixed-layout records read
  straight from the bytes.

## Python

The Blender extension's Python module, `zenkit_py`, is also published as a wheel, for scripts
that work on the game's files without Rust; [python.md](python.md) describes it.
