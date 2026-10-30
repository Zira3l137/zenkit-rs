//! Saves a world's mesh as a Wavefront `.obj` with its materials in an `.mtl` next to it, for
//! Blender and other 3D tools. The materials refer to their textures as `<texture>.png`, which
//! the `texture` example can make.
//!
//! ```text
//! cargo run --release --example world_obj -- "C:\Games\Gothic II" NEWWORLD.ZEN newworld.obj
//! ```
#![allow(clippy::print_stdout)]

mod common;

use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use zenkit::Error;
use zenkit::Result;
use zenkit::world::World;

fn main() -> ExitCode {
    let [gothic, name, output] = common::args("world_obj <gothic folder> <world> <output.obj>");
    common::init_logging();
    common::finish(run(Path::new(&gothic), &name, Path::new(&output)))
}

fn run(gothic: &Path, name: &str, output: &Path) -> Result<()> {
    let vfs = common::mount_game(gothic)?;
    let world: World = vfs.load(name)?;
    let mtl = output.with_extension("mtl");
    write_obj(&world, output, &mtl).map_err(Error::io_write(output))?;
    write_mtl(&world, &mtl).map_err(Error::io_write(&mtl))?;
    println!("saved {} and {}", output.display(), mtl.display());
    Ok(())
}

fn write_obj(world: &World, path: &Path, mtl: &Path) -> io::Result<()> {
    let mesh = &world.mesh;
    let mut out = BufWriter::new(File::create(path)?);
    let mtl_name = mtl.file_name().unwrap_or_default().to_string_lossy();
    writeln!(out, "mtllib {mtl_name}")?;

    // The engine works in centimetres in a left-handed space, where OBJ is right-handed:
    // mirroring Z and flipping the winding of every triangle keeps the world the right way
    // round. Texture rows also run top to bottom, against OBJ's bottom to top.
    for v in &mesh.vertices {
        writeln!(out, "v {} {} {}", v.x / 100.0, v.y / 100.0, -v.z / 100.0)?;
    }
    for f in &mesh.features {
        writeln!(out, "vt {} {}", f.texture.x, 1.0 - f.texture.y)?;
        writeln!(out, "vn {} {} {}", f.normal.x, f.normal.y, -f.normal.z)?;
    }

    let mut triangles: Vec<_> = world.triangles().collect();
    triangles.sort_by_key(|t| mesh.polygons[t.polygon].material);

    let mut material = None;
    for t in &triangles {
        let m = mesh.polygons[t.polygon].material;
        if material != Some(m) {
            writeln!(out, "usemtl {}", mesh.materials[m as usize].name)?;
            material = Some(m);
        }
        write!(out, "f")?;
        for corner in [0, 2, 1] {
            let (v, f) = (t.vertices[corner] + 1, t.features[corner] + 1);
            write!(out, " {v}/{f}/{f}")?;
        }
        writeln!(out)?;
    }
    out.flush()
}

fn write_mtl(world: &World, path: &Path) -> io::Result<()> {
    let mut out = BufWriter::new(File::create(path)?);
    for material in &world.mesh.materials {
        let c = material.color;
        writeln!(out, "newmtl {}", material.name)?;
        writeln!(out, "Kd {} {} {}", c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0)?;
        if let Some((stem, _)) = material.texture.rsplit_once('.') {
            writeln!(out, "map_Kd {stem}.png")?;
        }
        writeln!(out)?;
    }
    out.flush()
}
