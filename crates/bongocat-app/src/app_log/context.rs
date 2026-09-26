//! What travels with an event, bounded so a log line cannot grow without limit.
//!
//! Context is where a real path, a real model name or a real message would go,
//! and the product's rule is that the log carries none of those. So every field
//! here is bounded and marked, and a caller that has something longer passes a
//! summary. The count of fields is bounded too: a log line is one line, and a
//! context that wraps is a context nobody reads.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationLogContext {
    Operation(&'static str),
    Phase(&'static str),
    Service(&'static str),
    Reason(&'static str),
    State(&'static str),
    Source(&'static str),
    Result(&'static str),
    Count(u64),
    Bytes(u64),
    Revision(u64),
    PreviousLevel(&'static str),
    CurrentLevel(&'static str),
    PreviousRetentionDays(u64),
    CurrentRetentionDays(u64),
}

impl ApplicationLogContext {
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Operation(_) => "operation",
            Self::Phase(_) => "phase",
            Self::Service(_) => "service",
            Self::Reason(_) => "reason",
            Self::State(_) => "state",
            Self::Source(_) => "source",
            Self::Result(_) => "result",
            Self::Count(_) => "count",
            Self::Bytes(_) => "bytes",
            Self::Revision(_) => "revision",
            Self::PreviousLevel(_) => "previous_level",
            Self::CurrentLevel(_) => "current_level",
            Self::PreviousRetentionDays(_) => "previous_retention_days",
            Self::CurrentRetentionDays(_) => "current_retention_days",
        }
    }

    pub(crate) fn value(self) -> String {
        match self {
            Self::Operation(value)
            | Self::Phase(value)
            | Self::Service(value)
            | Self::Reason(value)
            | Self::State(value)
            | Self::Source(value)
            | Self::Result(value)
            | Self::PreviousLevel(value)
            | Self::CurrentLevel(value) => value.to_owned(),
            Self::Count(value)
            | Self::Bytes(value)
            | Self::Revision(value)
            | Self::PreviousRetentionDays(value)
            | Self::CurrentRetentionDays(value) => value.to_string(),
        }
    }
}
