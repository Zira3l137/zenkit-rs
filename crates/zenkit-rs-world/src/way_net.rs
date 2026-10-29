use tracing::warn;
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
