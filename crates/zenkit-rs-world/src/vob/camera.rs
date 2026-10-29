use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::io::Reader;
use zenkit_rs_core::math::Mat4;
use zenkit_rs_core::raw_enum;

use super::Ctx;
use super::VobId;

raw_enum! {
    /// How a camera eases into and out of a key frame.
    pub enum CameraMotion: u8 = Undefined {
        Undefined = 0,
        Smooth = 1,
        Linear = 2,
        Step = 3,
        Slow = 4,
        Fast = 5,
        Custom = 6,
    }
}

raw_enum! {
    /// What a camera path's coordinates are relative to.
    pub enum CameraReference: u8 = World {
        World = 0,
        /// The VOB the camera is attached to.
        Object = 1,
    }
}

raw_enum! {
    pub enum CameraLerp: u8 = Undefined {
        Undefined = 0,
        Path = 1,
        PathIgnoreRoll = 2,
        PathRotationSamples = 3,
    }
}

raw_enum! {
    pub enum CameraLoop: u8 = None {
        None = 0,
        Restart = 1,
        PingPong = 2,
    }
}

/// A camera that flies along a path during cutscenes.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CutsceneCamera {
    pub trajectory_for: CameraReference,
    pub target_trajectory_for: CameraReference,
    pub loop_mode: CameraLoop,
    pub lerp_mode: CameraLerp,
    pub ignore_for_vob_rotation: bool,
    pub ignore_for_vob_rotation_target: bool,
    pub adapt: bool,
    pub ease_first: bool,
    pub ease_last: bool,
    /// In seconds.
    pub total_duration: f32,
    pub auto_focus_vob: String,
    pub auto_player_movable: bool,
    pub auto_untrigger_last: bool,
    pub auto_untrigger_last_delay: f32,
    /// The key frames the camera moves through, which are [`CameraKeyFrame`]s.
    pub trajectory: Vec<VobId>,
    /// The key frames the camera looks at, which are [`CameraKeyFrame`]s.
    pub targets: Vec<VobId>,
    /// Gothic 2 save games only.
    pub state: Option<CameraState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CameraState {
    pub paused: bool,
    pub started: bool,
    pub goto_time_mode: bool,
    /// How far into the cutscene it is, in seconds.
    pub time: f32,
}

impl CutsceneCamera {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut camera = Self {
            trajectory_for: CameraReference::from_raw(r.enumeration()?),
            target_trajectory_for: CameraReference::from_raw(r.enumeration()?),
            loop_mode: CameraLoop::from_raw(r.enumeration()?),
            lerp_mode: CameraLerp::from_raw(r.enumeration()?),
            ignore_for_vob_rotation: r.bool()?,
            ignore_for_vob_rotation_target: r.bool()?,
            adapt: r.bool()?,
            ease_first: r.bool()?,
            ease_last: r.bool()?,
            total_duration: r.float()?,
            auto_focus_vob: r.name()?,
            auto_player_movable: r.bool()?,
            auto_untrigger_last: r.bool()?,
            auto_untrigger_last_delay: r.float()?,
            ..Self::default()
        };

        let positions = r.int()?;
        let targets = r.int()?;
        for _ in 0..positions {
            camera.trajectory.extend(r.vob()?);
        }
        for _ in 0..targets {
            camera.targets.extend(r.vob()?);
        }

        if r.save && r.game == GameVersion::Gothic2 {
            camera.state = Some(CameraState {
                paused: r.bool()?,
                started: r.bool()?,
                goto_time_mode: r.bool()?,
                time: r.float()?,
            });
        }
        Ok(camera)
    }
}

/// One key frame of a [`CutsceneCamera`]'s path. Its position and rotation are the VOB's.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CameraKeyFrame {
    /// In seconds from the start of the path.
    pub time: f32,
    /// In degrees.
    pub roll_angle: f32,
    pub fov_scale: f32,
    pub motion_type: CameraMotion,
    pub motion_type_fov: CameraMotion,
    pub motion_type_roll: CameraMotion,
    pub motion_type_time_scale: CameraMotion,
    pub tension: f32,
    pub bias: f32,
    pub continuity: f32,
    pub time_scale: f32,
    pub time_fixed: bool,
    /// The key frame's transform when the cutscene was made.
    pub original_pose: Mat4,
}

impl CameraKeyFrame {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            time: r.float()?,
            roll_angle: r.float()?,
            fov_scale: r.float()?,
            motion_type: CameraMotion::from_raw(r.enumeration()?),
            motion_type_fov: CameraMotion::from_raw(r.enumeration()?),
            motion_type_roll: CameraMotion::from_raw(r.enumeration()?),
            motion_type_time_scale: CameraMotion::from_raw(r.enumeration()?),
            tension: r.float()?,
            bias: r.float()?,
            continuity: r.float()?,
            time_scale: r.float()?,
            time_fixed: r.bool()?,
            original_pose: Reader::new(&r.raw(64)?).mat4()?,
        })
    }
}
