use tracing::warn;
use zenkit_rs_core::GameVersion;
use zenkit_rs_core::Result;
use zenkit_rs_core::math::Color;
use zenkit_rs_core::raw_enum;
use zenkit_rs_core::text;

use super::Ctx;

raw_enum! {
    pub enum LightType: u8 = Spot {
        Point = 0,
        Spot = 1,
        Reserved0 = 2,
        Reserved1 = 3,
    }
}

raw_enum! {
    pub enum LightQuality: u8 = Medium {
        High = 0,
        Medium = 1,
        /// The cheapest to render.
        Low = 2,
    }
}

/// A light source. Static lights are baked into the world's lightmaps; dynamic ones can be
/// switched and animated, so their fields from [`on`](Self::on) down are only stored for them.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    /// The light preset from `LIGHTPRESETS.ZEN` this light was made from.
    pub preset: String,
    pub light_type: LightType,
    pub range: f32,
    pub color: Color,
    /// In degrees, for [`LightType::Spot`].
    pub cone_angle: f32,
    pub is_static: bool,
    pub quality: LightQuality,
    pub lensflare_fx: String,
    pub on: bool,
    /// Factors the range is scaled by, one after another.
    pub range_animation_scale: Vec<f32>,
    pub range_animation_fps: f32,
    pub range_animation_smooth: bool,
    /// Colours the light runs through, one after another.
    pub color_animation_list: Vec<Color>,
    pub color_animation_fps: f32,
    pub color_animation_smooth: bool,
    /// Gothic 2 only.
    pub can_move: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self {
            preset: String::new(),
            light_type: LightType::Spot,
            range: 0.0,
            color: Color::default(),
            cone_angle: 0.0,
            is_static: false,
            quality: LightQuality::Medium,
            lensflare_fx: String::new(),
            on: false,
            range_animation_scale: Vec::new(),
            range_animation_fps: 0.0,
            range_animation_smooth: true,
            color_animation_list: Vec::new(),
            color_animation_fps: 0.0,
            color_animation_smooth: true,
            can_move: true,
        }
    }
}

impl Light {
    pub(crate) fn read(r: &mut Ctx) -> Result<Self> {
        let mut light = Self {
            preset: r.name()?,
            light_type: LightType::from_raw(r.enumeration()?),
            range: r.float()?,
            color: r.color()?,
            cone_angle: r.float()?,
            is_static: r.bool()?,
            quality: LightQuality::from_raw(r.enumeration()?),
            lensflare_fx: r.name()?,
            ..Self::default()
        };

        if !light.is_static {
            light.on = r.bool()?;
            light.range_animation_scale = scales(r.string()?);
            light.range_animation_fps = r.float()?;
            light.range_animation_smooth = r.bool()?;
            light.color_animation_list = colors(r.string()?);
            light.color_animation_fps = r.float()?;
            light.color_animation_smooth = r.bool()?;
            if r.game == GameVersion::Gothic2 {
                light.can_move = r.bool()?;
            }
        }
        Ok(light)
    }
}

/// Parses whitespace-separated numbers, stopping at the first that is not one.
fn scales(s: &[u8]) -> Vec<f32> {
    s.split(u8::is_ascii_whitespace).filter(|p| !p.is_empty()).map_while(text::float).collect()
}

/// Parses a list of colours, each a single number for a grey or three in parentheses, like
/// `128 (255 200 0) 64`.
fn colors(s: &[u8]) -> Vec<Color> {
    let mut colors = Vec::new();
    let mut rest = s.trim_ascii_start();
    while let Some(&first) = rest.first() {
        let channels = if first == b'(' {
            let end = rest.iter().position(|&b| b == b')').unwrap_or(rest.len());
            let parts = rest[1..end].split(u8::is_ascii_whitespace).filter(|p| !p.is_empty());
            let channels: Vec<_> = parts.map_while(text::int).collect();
            rest = rest.get(end + 1..).unwrap_or_default();
            channels
        } else {
            let len = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            let grey = text::int(&rest[..len]);
            rest = &rest[len..];
            grey.map(|v| vec![v; 3]).unwrap_or_default()
        };

        let [r, g, b] = channels[..] else {
            warn!("stopping at invalid colour animation list {:?}", text::latin1(s));
            break;
        };
        colors.push(Color::new(r as u8, g as u8, b as u8, 255));
        rest = rest.trim_ascii_start();
    }
    colors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_lists_mix_greys_and_triples() {
        let grey = |v| Color::new(v, v, v, 255);
        assert_eq!(colors(b""), []);
        assert_eq!(
            colors(b" 128 (255 200 0)(1 2 3) 64 "),
            [grey(128), Color::new(255, 200, 0, 255), Color::new(1, 2, 3, 255), grey(64)]
        );
        assert_eq!(colors(b"10 (1 2) 20"), [grey(10)]);
        assert_eq!(colors(b"10 x 20"), [grey(10)]);
    }

    #[test]
    fn scales_stop_at_the_first_non_number() {
        assert_eq!(scales(b" 1 0.5  2 x 3"), [1.0, 0.5, 2.0]);
    }
}
