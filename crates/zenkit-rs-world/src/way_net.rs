use tracing::warn;
use zenkit_rs_archive::ArchiveWriter;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Vec3;
use zenkit_rs_core::text;

use crate::vob::Ctx;
use crate::vob::Next;
use crate::vob::Shared;

/// A point of the way-net.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WayPoint {
    /// What scripts call it by, like `OW_PATH_1_17`.
    pub name: String,
    pub water_depth: i32,
    pub under_water: bool,
    pub position: Vec3,
    pub direction: Vec3,
    /// Whether the point is listed on its own rather than only as the end of a way. Free points
    /// can have no ways at all.
    pub free_point: bool,
}

impl WayPoint {
    /// Writes the point as an object and gives its index, for references to it.
    fn write(&self, w: &mut ArchiveWriter, key: &str) -> u32 {
        let index = w.begin_object(key, "zCWaypoint", 0);
        w.name("wpName", &self.name);
        w.int("waterDepth", self.water_depth);
        w.bool("underWater", self.under_water);
        w.vec3("position", self.position);
        w.vec3("direction", self.direction);
        w.end_object();
        index
    }
}

/// The graph of points and ways NPCs find their paths along.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WayNet {
    pub points: Vec<WayPoint>,
    /// The ways between points, as indices into [`points`](Self::points). NPCs walk them in both
    /// directions.
    pub ways: Vec<(usize, usize)>,
}

impl WayNet {
    /// Finds a point by name, ignoring case as the engine does.
    pub fn point(&self, name: &str) -> Option<(usize, &WayPoint)> {
        self.points.iter().enumerate().find(|(_, point)| point.name.eq_ignore_ascii_case(name))
    }

    /// Writes the way-net object: the free points, then each way as its two ends. A point is
    /// written whole the first time and as a reference after that, so every point must be free
    /// or the end of a way; one that is neither could not be stored.
    pub(crate) fn write(&self, w: &mut ArchiveWriter) {
        let mut used = vec![false; self.points.len()];
        for &(left, right) in &self.ways {
            for end in [left, right] {
                match used.get_mut(end) {
                    Some(used) => *used = true,
                    None => w.fail(format!("a way ends at point {end}, which does not exist")),
                }
            }
        }
        if let Some((point, _)) =
            self.points.iter().zip(&used).find(|(point, used)| !point.free_point && !**used)
        {
            w.fail(format!("way point {:?} is neither free nor the end of a way", point.name));
        }

        w.begin_object("%", "zCWayNet", 0);
        w.int("waynetVersion", 1);
        let free = self.points.iter().enumerate().filter(|(_, point)| point.free_point);
        w.int("numWaypoints", free.clone().count() as i32);
        let mut written = vec![None; self.points.len()];
        for (n, (i, point)) in free.enumerate() {
            written[i] = Some(point.write(w, &format!("waypoint{n}")));
        }

        w.int("numWays", self.ways.len() as i32);
        for (n, &(left, right)) in self.ways.iter().enumerate() {
            for (key, end) in [(format!("wayl{n}"), left), (format!("wayr{n}"), right)] {
                match (self.points.get(end), written.get(end).copied().flatten()) {
                    (_, Some(index)) => w.reference(&key, index),
                    (Some(point), None) => written[end] = Some(point.write(w, &key)),
                    (None, _) => w.null_object(&key),
                }
            }
        }
        w.end_object();
    }

    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let object = match r.begin("a way-net")? {
            Next::New(object) if object.class == b"zCWayNet" => object,
            Next::New(object) => {
                warn!("skipping way-net of class {:?}", text::latin1(object.class));
                r.skip_object()?;
                return Ok(Self::default());
            }
            _ => return Ok(Self::default()),
        };

        let mut net = Self::default();
        let _version = r.int()?;

        let count = r.int()?;
        for _ in 0..count {
            net.point_object(r, true)?;
        }

        let count = r.int()?;
        for i in 0..count {
            let left = net.point_object(r, false)?;
            let right = net.point_object(r, false)?;
            match (left, right) {
                (Some(left), Some(right)) => net.ways.push((left, right)),
                _ => warn!("skipping way {i}, which lacks an end"),
            }
        }

        r.end(&object)?;
        Ok(net)
    }

    /// Reads a waypoint, or a reference to one read before, and gives its index.
    fn point_object(&mut self, r: &mut Ctx, free_point: bool) -> Result<Option<usize>> {
        let object = match r.begin("a waypoint")? {
            Next::New(object) if object.class == b"zCWaypoint" => object,
            Next::New(object) => {
                warn!("skipping waypoint of class {:?}", text::latin1(object.class));
                r.skip_object()?;
                return Ok(None);
            }
            Next::Reference(Some(Shared::WayPoint(index))) => return Ok(Some(index)),
            Next::Reference(Some(other)) => {
                warn!("expected a reference to a waypoint, found one to {other:?}");
                return Ok(None);
            }
            Next::Null | Next::Reference(None) => return Ok(None),
        };

        let index = self.points.len();
        self.points.push(WayPoint {
            name: r.name()?,
            water_depth: r.int()?,
            under_water: r.bool()?,
            position: r.vec3()?,
            direction: r.vec3()?,
            free_point,
        });
        r.objects.insert(object.index, Shared::WayPoint(index));
        r.end(&object)?;
        Ok(Some(index))
    }
}
