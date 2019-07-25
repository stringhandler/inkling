use crate::{
    error::InklingError,
    line::{InternalChoice, InternalLine},
};

pub type FollowResult = Result<EncounteredEvent, InklingError>;

pub type LineDataBuffer = Vec<InternalLine>;

#[derive(Clone, Debug, PartialEq)]
/// What action that is prompted by following a story.
pub enum EncounteredEvent {
    /// Choice for the user.
    BranchingChoice(Vec<ChoiceInfo>),
    /// Divert to a new knot with the given name.
    Divert(String),
    /// Finished with the current node or story.
    Done,
}

#[derive(Clone, Debug, PartialEq)]
/// Information about a branching choice encountered in the story.
pub struct ChoiceInfo {
    /// Number of times that the branching node (not the choice itself) has been seen.
    pub num_visited: u32,
    /// Choice data to process before presenting to the user.
    pub choice_data: InternalChoice,
}
