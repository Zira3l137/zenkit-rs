//! Dumps every asset of one kind from a Gothic installation as JSON lines, in the same shape as
//! `dump_zenkit.py`, so `compare.py` can diff this crate against ZenKit.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rayon::prelude::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zenkit::Date;
use zenkit::animation::ModelAnimation;
use zenkit::material::Material;
use zenkit::math::{Aabb, Color, Mat4, Obb, Quat, Vec2, Vec3};
use zenkit::mesh::Mesh;
use zenkit::model::{Model, ModelHierarchy, ModelMesh};
use zenkit::morph::MorphMesh;
use zenkit::mrm::MultiResolutionMesh;
use zenkit::texture::Texture;
use zenkit::vfs::{Node, Overwrite, Vfs};

const USAGE: &str =
    "usage: zk-dump <mrm|msh|tex|mdh|mdm|mdl|mmb|man> <gothic directory> [output.jsonl]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(kind), Some(game)) = (args.first(), args.get(1)) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };

    let dump: fn(&[u8]) -> zenkit::Result<Value> = match kind.as_str() {
        "mrm" => |d| MultiResolutionMesh::from_bytes(d).map(|m| mrm(&m)),
        "msh" => |d| Mesh::from_bytes(d).map(|m| mesh(&m)),
        "tex" => |d| Texture::from_bytes(d).map(|t| texture(&t)),
        "mdh" => |d| ModelHierarchy::from_bytes(d).map(|h| hierarchy(&h)),
        "mdm" => |d| ModelMesh::from_bytes(d).map(|m| model_mesh(&m)),
        "mdl" => {
            |d| {
                Model::from_bytes(d).map(|m| json!({ "hierarchy": hierarchy(&m.hierarchy), "mesh": model_mesh(&m.mesh) }))
            }
        }
        "mmb" => |d| MorphMesh::from_bytes(d).map(|m| morph(&m)),
        "man" => |d| ModelAnimation::from_bytes(d).map(|a| animation(&a)),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match run(kind, Path::new(game), args.get(2).map(PathBuf::from), dump) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    kind: &str,
    game: &Path,
    output: Option<PathBuf>,
    dump: fn(&[u8]) -> zenkit::Result<Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    let disks = disks(&game.join("Data"))?;
    eprintln!("mounting {} archives", disks.len());

    let mut vfs = Vfs::new();
    vfs.mount_disks(&disks, Overwrite::Older)?;

    let mut files = Vec::new();
    collect(vfs.root(), String::new(), &format!(".{kind}"), &mut files);
    files.sort_by_key(|(path, _)| path.to_lowercase());
    eprintln!("dumping {} .{kind} files", files.len());

    let start = std::time::Instant::now();
    let lines: Vec<String> = files
        .par_iter()
        .map(|(path, node)| {
            let entry = match dump(node.data().unwrap_or_default()) {
                Ok(result) => json!({ "path": path, "result": result }),
                Err(e) => json!({ "path": path, "error": e.to_string() }),
            };
            entry.to_string()
        })
        .collect();
    eprintln!("parsed and serialised in {:.2?}", start.elapsed());

    let mut out: Box<dyn Write> = match output {
        Some(path) => Box::new(BufWriter::new(fs::File::create(path)?)),
        None => Box::new(BufWriter::new(std::io::stdout().lock())),
    };
    for line in lines {
        writeln!(out, "{line}")?;
    }
    out.flush()?;
    Ok(())
}

/// The `.vdf` files in `dir`, sorted by name so both dumpers mount them in the same order.
fn disks(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut disks: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("vdf")))
        .collect();
    disks.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    Ok(disks)
}

fn collect<'a>(dir: &'a Node, prefix: String, extension: &str, out: &mut Vec<(String, &'a Node)>) {
    for child in dir.children() {
        let path = format!("{prefix}{}", child.name());
        if child.is_dir() {
            collect(child, format!("{path}/"), extension, out);
        } else if path.to_lowercase().ends_with(extension) {
            out.push((path, child));
        }
    }
}

fn v2(v: Vec2) -> Value {
    json!([v.x, v.y])
}

fn v3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn quat(q: Quat) -> Value {
    json!([q.x, q.y, q.z, q.w])
}

fn mat4(m: Mat4) -> Value {
    json!(m.to_cols_array())
}

fn date(d: Date) -> Value {
    json!([d.year, d.month, d.day, d.hour, d.minute, d.second])
}

fn color(c: Color) -> Value {
    json!([c.r, c.g, c.b, c.a])
}

fn aabb(b: &Aabb) -> Value {
    json!({ "min": v3(b.min), "max": v3(b.max) })
}

fn obb(b: &Obb) -> Value {
    json!({
        "center": v3(b.center),
        "axes": b.axes.map(v3),
        "half_width": v3(b.half_width),
        "children": b.children.iter().map(obb).collect::<Vec<_>>(),
    })
}

fn material(m: &Material) -> Value {
    json!({
        "name": m.name,
        "group": m.group as u8,
        "color": color(m.color),
        "smooth_angle": m.smooth_angle,
        "texture": m.texture,
        "texture_scale": v2(m.texture_scale),
        "texture_anim_fps": m.texture_anim_fps,
        "texture_anim_map_mode": m.texture_anim_map_mode as u8,
        "texture_anim_map_dir": v2(m.texture_anim_map_dir),
        "disable_collision": m.disable_collision,
        "disable_lightmap": m.disable_lightmap,
        "dont_collapse": m.dont_collapse,
        "detail_object": m.detail_object,
        "detail_object_scale": m.detail_object_scale,
        "force_occluder": m.force_occluder,
        "environment_mapping": m.environment_mapping,
        "environment_mapping_strength": m.environment_mapping_strength,
        "wave_mode": m.wave_mode as u8,
        "wave_speed": m.wave_speed as u8,
        "wave_max_amplitude": m.wave_max_amplitude,
        "wave_grid_size": m.wave_grid_size,
        "ignore_sun": m.ignore_sun,
        "alpha_func": m.alpha_func as u8,
        "default_mapping": v2(m.default_mapping),
    })
}

fn mrm(m: &MultiResolutionMesh) -> Value {
    let sub_meshes: Vec<Value> = m
        .sub_meshes
        .iter()
        .map(|s| {
            json!({
                "material": material(&s.material),
                "triangles": s.triangles,
                "wedges": s.wedges.iter().map(|w| json!({
                    "normal": v3(w.normal), "texture": v2(w.texture), "index": w.index,
                })).collect::<Vec<_>>(),
                "colors": s.colors,
                "triangle_plane_indices": s.triangle_plane_indices,
                "triangle_planes": s.triangle_planes.iter().map(|p| json!({
                    "distance": p.distance, "normal": v3(p.normal),
                })).collect::<Vec<_>>(),
                "triangle_edges": s.triangle_edges,
                "edges": s.edges,
                "edge_scores": s.edge_scores,
                "wedge_map": s.wedge_map,
            })
        })
        .collect();

    json!({
        "positions": m.positions.iter().copied().map(v3).collect::<Vec<_>>(),
        "normals": m.normals.iter().copied().map(v3).collect::<Vec<_>>(),
        "alpha_test": m.alpha_test,
        "bbox": aabb(&m.bbox),
        "obb": obb(&m.obb),
        "sub_meshes": sub_meshes,
    })
}

fn mesh(m: &Mesh) -> Value {
    let polygons: Vec<Value> = m
        .polygons
        .iter()
        .map(|p| {
            let f = p.flags;
            json!({
                "material": p.material,
                "lightmap": p.lightmap.map_or(-1, i32::from),
                "vertices": m.polygon_vertices(p),
                "features": m.polygon_features(p),
                "is_portal": f.is_portal(),
                "is_occluder": f.is_occluder,
                "is_sector": f.is_sector,
                "should_relight": f.should_relight,
                "is_outdoor": f.is_outdoor,
                "is_ghost_occluder": f.is_ghost_occluder,
                "is_dynamically_lit": f.is_dynamically_lit,
                "is_lod": f.is_lod,
                "normal_axis": f.normal_axis,
                "sector_index": f.sector_index,
            })
        })
        .collect();

    json!({
        "name": m.name,
        "date": date(m.date),
        "bbox": aabb(&m.bbox),
        "obb": obb(&m.obb),
        "materials": m.materials.iter().map(material).collect::<Vec<_>>(),
        "vertices": m.vertices.iter().copied().map(v3).collect::<Vec<_>>(),
        "features": m.features.iter().map(|f| json!({
            "texture": v2(f.texture), "light": f.light, "normal": v3(f.normal),
        })).collect::<Vec<_>>(),
        "lightmaps": m.lightmaps.iter().map(|l| json!({
            "texture": texture(&m.lightmap_textures[l.texture]),
            "origin": v3(l.origin),
            "normals": l.normals.map(v3),
        })).collect::<Vec<_>>(),
        "polygons": polygons,
    })
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn texture(t: &Texture) -> Value {
    let c = t.average_color;
    let argb = u32::from_be_bytes([c.a, c.r, c.g, c.b]);
    let mipmaps: Vec<Value> = (0..t.mipmap_count())
        .map(|level| {
            let (width, height) = t.mipmap_dimensions(level);
            json!({
                "width": width,
                "height": height,
                "raw": sha256(t.mipmap(level).unwrap_or_default()),
                "rgba": t.to_rgba8(level).ok().map(|p| sha256(&p)),
            })
        })
        .collect();

    json!({
        "format": t.format as u32,
        "width": t.width,
        "height": t.height,
        "reference_width": t.reference_width,
        "reference_height": t.reference_height,
        "average_color": argb,
        "mipmaps": mipmaps,
    })
}

fn hierarchy(h: &ModelHierarchy) -> Value {
    json!({
        "nodes": h.nodes.iter().map(|n| json!({
            "name": n.name,
            "parent": n.parent.map_or(-1, i32::from),
            "transform": mat4(n.transform),
        })).collect::<Vec<_>>(),
        "bbox": aabb(&h.bbox),
        "collision_bbox": aabb(&h.collision_bbox),
        "root_translation": v3(h.root_translation),
        "checksum": h.checksum,
        "source_date": date(h.source_date),
        "source_path": h.source_path,
    })
}

fn model_mesh(m: &ModelMesh) -> Value {
    let meshes: Vec<Value> = m
        .meshes
        .iter()
        .map(|s| {
            json!({
                "mesh": mrm(&s.mesh),
                "weights": s.weights.iter().map(|ws| ws.iter().map(|w| json!({
                    "weight": w.weight, "position": v3(w.position), "node": w.node,
                })).collect::<Vec<_>>()).collect::<Vec<_>>(),
                "wedge_normals": s.wedge_normals.iter().map(|n| json!({
                    "normal": v3(n.normal), "index": n.index,
                })).collect::<Vec<_>>(),
                "nodes": s.nodes,
                "bboxes": s.bboxes.iter().map(obb).collect::<Vec<_>>(),
            })
        })
        .collect();

    // Keyed by node name: ZenKit keeps attachments in a hash map, so their order means nothing.
    let attachments: serde_json::Map<String, Value> = m
        .attachments
        .iter()
        .map(|a| (a.node.clone(), mrm(&a.mesh)))
        .collect();

    json!({ "checksum": m.checksum, "meshes": meshes, "attachments": attachments })
}

fn morph(m: &MorphMesh) -> Value {
    json!({
        "name": m.name,
        "mesh": mrm(&m.mesh),
        "morph_positions": m.morph_positions.iter().copied().map(v3).collect::<Vec<_>>(),
        "animations": m.animations.iter().map(|a| json!({
            "name": a.name,
            "layer": a.layer,
            "blend_in": a.blend_in,
            "blend_out": a.blend_out,
            "duration": a.duration,
            "speed": a.speed,
            "flags": a.flags,
            "frame_count": a.frame_count,
            "vertices": a.vertices,
            "samples": a.samples.iter().copied().map(v3).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "sources": m.sources.iter().map(|s| json!({
            "date": date(s.date), "file_name": s.file_name,
        })).collect::<Vec<_>>(),
    })
}

fn animation(a: &ModelAnimation) -> Value {
    json!({
        "name": a.name,
        "next": a.next,
        "layer": a.layer,
        "frame_count": a.frame_count,
        "node_count": a.node_count,
        "fps": a.fps,
        "fps_source": a.fps_source,
        "sample_position_min": a.sample_position_min,
        "sample_position_scale": a.sample_position_scale,
        "bbox": aabb(&a.bbox),
        "checksum": a.checksum,
        "source_path": a.source_path,
        "source_date": date(a.source_date),
        "source_script": a.source_script,
        "samples": a.samples.iter().map(|s| json!({
            "position": v3(s.position), "rotation": quat(s.rotation),
        })).collect::<Vec<_>>(),
        "events": a.events.iter().map(|e| json!({
            "kind": e.kind as u32,
            "frame": e.frame,
            "tag": e.tag,
            "content": e.content,
            "values": e.values,
            "probability": e.probability,
        })).collect::<Vec<_>>(),
        "node_indices": a.node_indices,
    })
}
