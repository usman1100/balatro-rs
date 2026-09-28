#[cfg(feature = "python")]
use pyo3::exceptions::PyException;
#[cfg(feature = "python")]
use pyo3::prelude::*;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum PlayHandError {
    #[error("Played hand contains more than 5 cards")]
    TooManyCards,
    #[error("Played hand contains no cards")]
    NoCards,
    #[error("Played hand could not determine best hand")]
    UnknownHand,
    /// The Eye: the same hand type was played last this blind.
    #[error("The Eye forbids repeating a hand type in a row")]
    EyeRepeatRank,
    /// The Mouth: only the first played hand's type is legal.
    #[error("The Mouth only allows the first played hand's type")]
    MouthRankMismatch,
    /// The Psychic: every hand must be exactly five cards.
    #[error("The Psychic requires exactly 5 cards")]
    PsychicWrongCardCount,
}

#[derive(Error, Debug, Clone)]
pub enum GameError {
    #[error("No remaining discards")]
    NoRemainingDiscards,
    #[error("No remaining plays")]
    NoRemainingPlays,
    #[error("Invalid hand played")]
    InvalidHand(#[from] PlayHandError),
    #[error("Invalid stage")]
    InvalidStage,
    #[error("Invalid action")]
    InvalidAction,
    #[error("No blind match")]
    InvalidBlind,
    #[error("No card match")]
    NoCardMatch,
    #[error("No joker match")]
    NoJokerMatch,
    #[error("No consumable match")]
    NoConsumableMatch,
    #[error("No voucher match")]
    NoVoucherMatch,
    #[error("Voucher already redeemed")]
    VoucherAlreadyRedeemed,
    #[error("Invalid move direction")]
    InvalidMoveDirection,
    #[error("No available slot")]
    NoAvailableSlot,
    #[error("Invalid balance")]
    InvalidBalance,
    #[error("Invalid select card")]
    InvalidSelectCard,
    #[error("Invalid action space")]
    InvalidActionSpace,
    /// The Cerulean Bell's pinned card cannot be deselected.
    #[error("This card is forced to stay selected by The Cerulean Bell")]
    ForcedCardStaysSelected,
}

impl std::convert::From<ActionSpaceError> for GameError {
    fn from(_err: ActionSpaceError) -> GameError {
        GameError::InvalidActionSpace
    }
}

#[cfg(feature = "python")]
impl std::convert::From<GameError> for PyErr {
    fn from(err: GameError) -> PyErr {
        PyException::new_err(err.to_string())
    }
}

#[derive(Error, Debug, Clone)]
pub enum ActionSpaceError {
    #[error("Invalid index")]
    InvalidIndex,
    #[error("Invalid conversion to action")]
    InvalidActionConversion,
    #[error("Masked action")]
    MaskedAction,
}
