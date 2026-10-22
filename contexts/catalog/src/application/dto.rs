#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Principal {
    pub principal_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WorkloadIdentity {
    Processes,
    Untrusted,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandContext {
    pub caller_scope: String,
    pub principal: Option<Principal>,
    pub idempotency_key: String,
    pub correlation_id: String,
    pub consistency_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MoneyInput {
    pub minor: u64,
    pub currency_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ClosingPolicyInput {
    pub scheduled_end_millis: u64,
    pub inactivity_millis: Option<u64>,
    pub hard_deadline_millis: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImageAttachmentInput {
    pub image_id: String,
    pub alt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ListLotInput {
    pub context: CommandContext,
    pub lot_id: String,
    pub title: String,
    pub description: Option<String>,
    pub images: Vec<ImageAttachmentInput>,
    pub start_price: MoneyInput,
    pub bid_step_minor: u64,
    pub closing_policy: ClosingPolicyInput,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateLotInput {
    pub context: CommandContext,
    pub lot_id: String,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub expected_version: Option<u64>,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WithdrawLotInput {
    pub context: CommandContext,
    pub caller: WorkloadIdentity,
    pub lot_id: String,
    pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LotMutationResult {
    pub lot_id: String,
    pub version: u64,
    pub checked_at_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LotCommandResult {
    pub lot_id: String,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RequestUploadInput {
    pub context: CommandContext,
    pub content_type: String,
    pub content_length: u64,
    pub checksum_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UploadIntentResult {
    pub image_id: String,
    pub upload_url: String,
    pub expires_at_millis: u64,
    pub required_headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompleteUploadInput {
    pub context: CommandContext,
    pub image_id: String,
    pub now_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UploadCompletedResult {
    pub image_id: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicLotImage {
    pub image_id: String,
    pub url: String,
    pub alt: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicLot {
    pub lot_id: String,
    pub title: String,
    pub description: Option<String>,
    pub images: Vec<PublicLotImage>,
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LotPage {
    pub lots: Vec<PublicLot>,
    pub cursors: Vec<String>,
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}
