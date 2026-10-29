use zenkit_rs_core::Result;
use zenkit_rs_core::raw_enum;

use super::Ctx;

raw_enum! {
    pub enum SoundMode: u8 = Once {
        /// Plays over and over while the player is in range.
        Loop = 0,
        /// Plays once each time the player comes into range.
        Once = 1,
        /// Plays now and then while the player is in range, see [`Sound::random_delay`].
        Random = 2,
    }
}

raw_enum! {
    /// The shape of the volume a sound is heard in.
    pub enum SoundVolume: u8 = Spherical {
        Spherical = 0,
        Ellipsoidal = 1,
    }
}

/// A sound source.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// From 0 to 100.
    pub volume: f32,
    pub mode: SoundMode,
    /// In seconds, the average pause between plays in [`SoundMode::Random`].
    pub random_delay: f32,
    /// In seconds, how far a pause may stray from [`random_delay`](Self::random_delay).
    pub random_delay_var: f32,
    pub initially_playing: bool,
    pub ambient_3d: bool,
    /// Whether walls muffle the sound.
    pub obstruction: bool,
    pub cone_angle: f32,
    pub volume_type: SoundVolume,
    pub radius: f32,
    /// A `.WAV` file or an instance from the sound scripts.
    pub sound_name: String,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            volume: 0.0,
            mode: SoundMode::Once,
            random_delay: 0.0,
            random_delay_var: 0.0,
            initially_playing: false,
            ambient_3d: false,
            obstruction: true,
            cone_angle: 0.0,
            volume_type: SoundVolume::Spherical,
            radius: 0.0,
            sound_name: String::new(),
        }
    }
}

impl Sound {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            volume: r.float()?,
            mode: SoundMode::from_raw(r.enumeration()?),
            random_delay: r.float()?,
            random_delay_var: r.float()?,
            initially_playing: r.bool()?,
            ambient_3d: r.bool()?,
            obstruction: r.bool()?,
            cone_angle: r.float()?,
            volume_type: SoundVolume::from_raw(r.enumeration()?),
            radius: r.float()?,
            sound_name: r.name()?,
        })
    }
}

/// A sound that changes with the time of day: the base sound plays between the start and end
/// time, [`sound_name2`](Self::sound_name2), if set, the rest of the day.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SoundDaytime {
    pub sound: Sound,
    /// In hours.
    pub start_time: f32,
    /// In hours.
    pub end_time: f32,
    pub sound_name2: String,
}

impl SoundDaytime {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        Ok(Self {
            sound: Sound::read(r)?,
            start_time: r.float()?,
            end_time: r.float()?,
            sound_name2: r.name()?,
        })
    }
}
