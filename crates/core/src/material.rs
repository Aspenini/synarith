//! Physically based material channels declared by a `material { ... }` block.
use crate::Type;
use std::fmt;

/// How an output's values are meant to be read and stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    /// Linear premultiplied color for display; 8-bit storage encodes straight sRGB.
    Color,
    /// Linear data such as roughness, height, or an encoded normal; 8-bit storage
    /// keeps values linear, without sRGB encoding or unpremultiplication.
    Data,
}

/// One physically based surface channel.
///
/// Channels follow the metallic-roughness model used by glTF and most real-time
/// engines. Every channel except [`Channel::Normal`] is written in the document;
/// the normal is derived from [`Channel::Height`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    /// Base color (`color`, required). Alpha is surface opacity.
    Albedo,
    /// Tangent-space normal derived from the height channel, encoded `n * 0.5 + 0.5`
    /// with +Y pointing up the image (the OpenGL and glTF convention).
    Normal,
    /// Surface height (`float`) in document units, the same units as `p`; it is the
    /// true relief, so its slopes set the normal.
    Height,
    /// Perceptual roughness (`float`, 0 smooth to 1 rough).
    Roughness,
    /// Metalness (`float`, 0 dielectric to 1 metal).
    Metallic,
    /// Ambient occlusion (`float`, 0 fully occluded to 1 open).
    Occlusion,
    /// Emitted light (`color`, linear, may exceed one).
    Emissive,
}

impl Channel {
    /// Every channel in canonical order.
    pub const ALL: [Channel; 7] = [
        Channel::Albedo,
        Channel::Normal,
        Channel::Height,
        Channel::Roughness,
        Channel::Metallic,
        Channel::Occlusion,
        Channel::Emissive,
    ];
    /// The channel's name inside a `material` block.
    pub fn name(self) -> &'static str {
        match self {
            Channel::Albedo => "albedo",
            Channel::Normal => "normal",
            Channel::Height => "height",
            Channel::Roughness => "roughness",
            Channel::Metallic => "metallic",
            Channel::Occlusion => "occlusion",
            Channel::Emissive => "emissive",
        }
    }
    /// Find a channel by its block name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
    /// The output name the channel compiles to, such as `material.albedo`.
    pub fn output_name(self) -> String {
        format!("material.{}", self.name())
    }
    /// The type the document must give the channel. The normal is derived, not written.
    pub fn ty(self) -> Type {
        match self {
            Channel::Albedo | Channel::Emissive | Channel::Normal => Type::Color,
            _ => Type::Float,
        }
    }
    /// How the channel's values are stored.
    pub fn kind(self) -> OutputKind {
        match self {
            Channel::Albedo | Channel::Emissive => OutputKind::Color,
            _ => OutputKind::Data,
        }
    }
}
impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The channels a document's material provides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Material {
    /// Present channels in canonical order, including a derived normal.
    pub channels: Vec<Channel>,
}
impl Material {
    /// Whether the material provides `channel`.
    pub fn has(&self, channel: Channel) -> bool {
        self.channels.contains(&channel)
    }
    /// Channels a fused material render evaluates, in plane order: every present
    /// channel except the normal, which is derived from the height.
    pub fn evaluated(&self) -> Vec<Channel> {
        self.planes().concat()
    }
    /// The planes a fused material render writes, and the channels in each.
    ///
    /// Each color channel (albedo, then emissive) fills a plane of RGBA. The
    /// scalar channels present (height, roughness, metallic, occlusion, in that
    /// order) share one final plane, one per component; unused components are zero.
    pub fn planes(&self) -> Vec<Vec<Channel>> {
        let (colors, scalars): (Vec<Channel>, Vec<Channel>) = self
            .channels
            .iter()
            .copied()
            .filter(|c| *c != Channel::Normal)
            .partition(|c| c.ty() == crate::Type::Color);
        let mut planes: Vec<Vec<Channel>> = colors.into_iter().map(|c| vec![c]).collect();
        if !scalars.is_empty() {
            planes.push(scalars);
        }
        planes
    }
    /// Pixels of border a fused render evaluates around each region: one when the
    /// normal is derived from neighboring heights, otherwise zero.
    pub fn apron(&self) -> u32 {
        u32::from(self.has(Channel::Normal))
    }
}
