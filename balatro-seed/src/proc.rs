//! Per-effect probability rolls.
//!
//! Balatro's one-off effect procs are not drawn from a shared stream — each
//! calls `pseudorandom('<key>')`, which is its own entry in the per-node
//! cache. That makes each one individually reproducible, so each gets a node
//! here rather than a single generic "roll" stream.
//!
//! Keys were transcribed from the game's Lua (`card.lua`, `blind.lua`) at
//! [`NODE_SOURCE_VERSION`], while `InstParams::version` targets 1.0.1f. The
//! transcription is therefore unverified against the targeted version: diff
//! these keys against a 1.0.1f dump before trusting them, and expect only
//! additions there, not renames.

use std::fmt;

/// The game version [`ProcNode`]'s keys were transcribed from.
pub const NODE_SOURCE_VERSION: i64 = 10000;

/// One node per probabilistic effect. `Display` must match the game's
/// node-ID string exactly, it is hashed to seed the per-decision RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcNode {
    /// Lucky Card: +20 Mult.
    LuckyMult,
    /// Lucky Card: +$20.
    LuckyMoney,
    /// Misprint's flat Mult range.
    Misprint,
    /// Hologram, which only triggers on the ante it was bought in.
    Hologram(i32),
    EightBall,
    Bloodstone,
    ReservedParking,
    SpaceJoker,
    BusinessCard,
    GrosMichel,
    Cavendish,
    /// The Wheel of Fortune tarot.
    WheelOfFortune,
    /// The Certificate tarot's seal type.
    CertificateSeal,
    /// The Wheel boss blind's 1-in-7 edition roll.
    WheelBoss,
    /// The Crimson Heart boss blind's pick of which Joker to debuff.
    CrimsonHeart,
}

impl fmt::Display for ProcNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LuckyMult => f.write_str("lucky_mult"),
            Self::LuckyMoney => f.write_str("lucky_money"),
            Self::Misprint => f.write_str("misprint"),
            Self::Hologram(ante) => write!(f, "halu{ante}"),
            Self::EightBall => f.write_str("8ball"),
            Self::Bloodstone => f.write_str("bloodstone"),
            Self::ReservedParking => f.write_str("parking"),
            Self::SpaceJoker => f.write_str("space"),
            Self::BusinessCard => f.write_str("business"),
            Self::GrosMichel => f.write_str("gros_michel"),
            Self::Cavendish => f.write_str("cavendish"),
            Self::WheelOfFortune => f.write_str("wheel_of_fortune"),
            Self::CertificateSeal => f.write_str("certsl"),
            Self::WheelBoss => f.write_str("wheel"),
            Self::CrimsonHeart => f.write_str("crimson_heart"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins every key against the string transcribed from the game's Lua.
    #[test]
    fn proc_node_strings_match_the_game() {
        let cases: &[(ProcNode, &str)] = &[
            (ProcNode::LuckyMult, "lucky_mult"),
            (ProcNode::LuckyMoney, "lucky_money"),
            (ProcNode::Misprint, "misprint"),
            (ProcNode::Hologram(4), "halu4"),
            (ProcNode::EightBall, "8ball"),
            (ProcNode::Bloodstone, "bloodstone"),
            (ProcNode::ReservedParking, "parking"),
            (ProcNode::SpaceJoker, "space"),
            (ProcNode::BusinessCard, "business"),
            (ProcNode::GrosMichel, "gros_michel"),
            (ProcNode::Cavendish, "cavendish"),
            (ProcNode::WheelOfFortune, "wheel_of_fortune"),
            (ProcNode::CertificateSeal, "certsl"),
            (ProcNode::WheelBoss, "wheel"),
            (ProcNode::CrimsonHeart, "crimson_heart"),
        ];
        for (node, expected) in cases {
            assert_eq!(node.to_string(), *expected);
        }
    }
}
