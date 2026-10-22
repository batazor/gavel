use std::{error::Error, fmt};

use super::{DurationMillis, Timestamp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosingPolicy {
    scheduled_end: Option<Timestamp>,
    inactivity: Option<DurationMillis>,
    hard_deadline: Option<Timestamp>,
}

impl ClosingPolicy {
    pub fn new(
        scheduled_end: Option<Timestamp>,
        inactivity: Option<DurationMillis>,
        hard_deadline: Option<Timestamp>,
    ) -> Result<Self, ClosingPolicyError> {
        if scheduled_end.is_none() && inactivity.is_none() {
            return Err(ClosingPolicyError::MissingEndCondition);
        }
        if matches!((scheduled_end, hard_deadline), (Some(end), Some(hard)) if end > hard) {
            return Err(ClosingPolicyError::ScheduledEndAfterHardDeadline);
        }

        Ok(Self {
            scheduled_end,
            inactivity,
            hard_deadline,
        })
    }

    pub const fn scheduled_end(&self) -> Option<Timestamp> {
        self.scheduled_end
    }

    pub const fn inactivity(&self) -> Option<DurationMillis> {
        self.inactivity
    }

    pub const fn hard_deadline(&self) -> Option<Timestamp> {
        self.hard_deadline
    }

    pub fn initial_end(&self, opened_at: Timestamp) -> Result<Timestamp, ClosingPolicyError> {
        self.end_for(opened_at)
    }

    pub fn end_after_bid(&self, bid_at: Timestamp) -> Result<Timestamp, ClosingPolicyError> {
        self.end_for(bid_at)
    }

    pub fn rescheduled(&self, scheduled_end: Timestamp) -> Result<Self, ClosingPolicyError> {
        Self::new(Some(scheduled_end), self.inactivity, self.hard_deadline)
    }

    fn end_for(&self, reference: Timestamp) -> Result<Timestamp, ClosingPolicyError> {
        let inactivity_end = self
            .inactivity
            .map(|duration| {
                reference
                    .checked_add(duration)
                    .ok_or(ClosingPolicyError::TimestampOverflow)
            })
            .transpose()?;

        let computed = match (self.scheduled_end, inactivity_end) {
            (Some(scheduled), Some(inactive)) => scheduled.max(inactive),
            (Some(scheduled), None) => scheduled,
            (None, Some(inactive)) => inactive,
            (None, None) => return Err(ClosingPolicyError::MissingEndCondition),
        };

        Ok(self
            .hard_deadline
            .map_or(computed, |hard_deadline| computed.min(hard_deadline)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClosingPolicyError {
    MissingEndCondition,
    ScheduledEndAfterHardDeadline,
    TimestampOverflow,
}

impl fmt::Display for ClosingPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEndCondition => {
                write!(formatter, "scheduled end or inactivity window is required")
            }
            Self::ScheduledEndAfterHardDeadline => {
                write!(formatter, "scheduled end cannot exceed the hard deadline")
            }
            Self::TimestampOverflow => write!(formatter, "calculated deadline overflows timestamp"),
        }
    }
}

impl Error for ClosingPolicyError {}
