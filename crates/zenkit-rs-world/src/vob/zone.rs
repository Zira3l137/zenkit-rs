use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Color;

use super::Ctx;

/// A volume that plays music from the soundtrack while the player is inside.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ZoneMusic {
    pub enabled: bool,
    /// Which zone wins where zones overlap.
    pub priority: i32,
    /// Whether the volume is an ellipsoid rather than its bounding box.
    pub ellipsoid: bool,
    pub reverb: f32,
    pub volume: f32,
    pub looping: bool,
    /// Save games only.
    pub state: Option<ZoneMusicState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ZoneMusicState {
    pub local_enabled: bool,
    pub day_entrance_done: bool,
    pub night_entrance_done: bool,
}

impl ZoneMusic {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut zone = Self {
            enabled: r.bool()?,
            priority: r.int()?,
            ellipsoid: r.bool()?,
            reverb: r.float()?,
            volume: r.float()?,
            looping: r.bool()?,
            state: None,
        };
        if r.save {
            zone.state = Some(ZoneMusicState {
                local_enabled: r.bool()?,
                day_entrance_done: r.bool()?,
                night_entrance_done: r.bool()?,
            });
        }
        Ok(zone)
    }
}

/// A volume that changes the fog while the camera is inside.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ZoneFog {
    pub range_center: f32,
    pub inner_range_percentage: f32,
    pub color: Color,
    /// Gothic 2 only, like [`override_color`](Self::override_color).
    pub fade_out_sky: bool,
    pub override_color: bool,
}

impl ZoneFog {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut fog = Self {
            range_center: r.float()?,
            inner_range_percentage: r.float()?,
            color: r.color()?,
            ..Self::default()
        };
        if r.game == GameVersion::Gothic2 {
            fog.fade_out_sky = r.bool()?;
            fog.override_color = r.bool()?;
        }
        Ok(fog)
    }
}

/// A volume that changes the view distance while the camera is inside.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ZoneFarPlane {
    pub vob_far_plane_z: f32,
    pub inner_range_percentage: f32,
}

impl ZoneFarPlane {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self { vob_far_plane_z: r.float()?, inner_range_percentage: r.float()? })
    }
}
