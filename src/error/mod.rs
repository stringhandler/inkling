//! Errors from creating or walking through stories.

#[macro_use]
mod error;
mod parse;

pub(crate) use error::IncorrectNodeStackError;
pub use error::InklingError;
pub use parse::ParseError;

pub(crate) use error::{InternalError, ProcessError, ProcessErrorKind, StackError};
pub(crate) use parse::{InvalidAddressError, KnotError, KnotNameError, LineErrorKind, LineParsingError};
