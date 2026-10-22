use std::fmt;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

string_id!(AuctionId);
string_id!(LotId);
string_id!(SellerId);
string_id!(BidderId);
string_id!(BidId);
string_id!(PaymentId);

/// Public identity which is stable only inside one auction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BidderAlias(u64);

impl BidderAlias {
    pub fn from_sequence(sequence: u64) -> Self {
        Self(sequence)
    }

    pub fn sequence(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for BidderAlias {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "bidder-{}", self.0)
    }
}

/// Milliseconds since Unix epoch. Time is always supplied by a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    pub const fn from_millis(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    pub fn checked_add(self, duration: DurationMillis) -> Option<Self> {
        self.0.checked_add(duration.0).map(Self)
    }
}

/// A strictly positive duration expressed in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurationMillis(u64);

impl DurationMillis {
    pub fn new(value: u64) -> Option<Self> {
        (value > 0).then_some(Self(value))
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }
}
