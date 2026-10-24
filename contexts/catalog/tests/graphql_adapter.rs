use std::sync::{Arc, Mutex};

use async_graphql::{Request, Variables, value};
use async_trait::async_trait;
use gavel_catalog::{
    adapters::inbound::graphql::{CatalogGraphqlApi, VerifiedRequestContext, schema},
    application::{
        ApplicationError, CompleteUploadInput, ListLotInput, LotPage, Principal, PublicLot,
        PublicLotImage, RequestUploadInput, UpdateLotInput, UploadCompletedResult,
        UploadIntentResult,
    },
};

#[derive(Default)]
struct MockApi {
    requests: Mutex<Vec<RequestUploadInput>>,
    listings: Mutex<Vec<ListLotInput>>,
    error: Mutex<Option<ApplicationError>>,
}

fn lot(id: &str) -> PublicLot {
    PublicLot {
        lot_id: id.to_owned(),
        title: "Vintage clock".to_owned(),
        description: Some("Works".to_owned()),
        images: vec![PublicLotImage {
            image_id: "image-1".to_owned(),
            url: "https://media.example.test/image-1".to_owned(),
            alt: "front".to_owned(),
        }],
        version: 1,
    }
}

#[async_trait]
impl CatalogGraphqlApi for MockApi {
    async fn lot(&self, lot_id: &str) -> Result<Option<PublicLot>, ApplicationError> {
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok((lot_id != "hidden").then(|| lot(lot_id)))
    }

    async fn lots(
        &self,
        _first: usize,
        _after: Option<String>,
    ) -> Result<LotPage, ApplicationError> {
        Ok(LotPage {
            lots: vec![lot("lot-1")],
            cursors: vec!["cursor-1".to_owned()],
            end_cursor: Some("cursor-1".to_owned()),
            has_next_page: false,
        })
    }

    async fn request_upload(
        &self,
        input: RequestUploadInput,
    ) -> Result<UploadIntentResult, ApplicationError> {
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        self.requests.lock().unwrap().push(input);
        Ok(UploadIntentResult {
            image_id: "image-1".to_owned(),
            upload_url: "https://storage.example.test/signed".to_owned(),
            expires_at_millis: 1_700_000_060_000,
            required_headers: vec![("x-checksum".to_owned(), "required".to_owned())],
        })
    }

    async fn complete_upload(
        &self,
        input: CompleteUploadInput,
    ) -> Result<UploadCompletedResult, ApplicationError> {
        Ok(UploadCompletedResult {
            image_id: input.image_id,
            status: "READY".to_owned(),
        })
    }

    async fn list_lot(&self, input: ListLotInput) -> Result<PublicLot, ApplicationError> {
        self.listings.lock().unwrap().push(input);
        Ok(lot("lot-created"))
    }

    async fn update_lot(&self, input: UpdateLotInput) -> Result<PublicLot, ApplicationError> {
        Ok(lot(&input.lot_id))
    }
}

fn verified() -> VerifiedRequestContext {
    VerifiedRequestContext {
        principal: Principal {
            principal_id: "seller-1".to_owned(),
        },
        correlation_id: "correlation-1".to_owned(),
        now_millis: 1_700_000_000_000,
    }
}

#[tokio::test]
async fn queries_map_public_delivery_shape_cursor_pages_and_hidden_lot() {
    let response = schema(Arc::new(MockApi::default()))
        .execute(
            "{ lot(id: \"lot-1\") { id title description images { id url alt } } \
             hidden: lot(id: \"hidden\") { id } \
             lots(first: 1) { edges { cursor node { id } } pageInfo { endCursor hasNextPage } } }",
        )
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.data,
        value!({
            "lot": {"id":"lot-1", "title":"Vintage clock", "description":"Works", "images":[{
                "id":"image-1", "url":"https://media.example.test/image-1", "alt":"front"
            }]},
            "hidden": null,
            "lots": {"edges":[{"cursor":"cursor-1", "node":{"id":"lot-1"}}],
                     "pageInfo":{"endCursor":"cursor-1", "hasNextPage":false}}
        })
    );
    let serialized = serde_json::to_string(&response).unwrap();
    for private in ["object_key", "authz", "receipt", "checksum"] {
        assert!(!serialized.contains(private));
    }
}

#[tokio::test]
async fn mutations_require_verified_extension_and_ignore_spoofable_arguments() {
    let api = Arc::new(MockApi::default());
    let document = "mutation Upload($input: RequestLotImageUploadInput!, $key: String!) { \
        requestLotImageUpload(input: $input, idempotencyKey: $key) { imageId expiresAt requiredHeaders { name value } } }";
    let variables = Variables::from_value(value!({
        "input":{"contentType":"image/png", "contentLength":42, "checksumSha256":"digest"},
        "key":"upload-1"
    }));
    let unauthenticated = schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>)
        .execute(Request::new(document).variables(variables.clone()))
        .await;
    assert_eq!(
        unauthenticated.errors[0]
            .extensions
            .as_ref()
            .unwrap()
            .get("code"),
        Some(&value!("UNAUTHENTICATED"))
    );
    assert!(api.requests.lock().unwrap().is_empty());

    let response = schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>)
        .execute(Request::new(document).variables(variables).data(verified()))
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    {
        let requests = api.requests.lock().unwrap();
        let request = &requests[0];
        assert_eq!(
            request.context.principal.as_ref().unwrap().principal_id,
            "seller-1"
        );
        assert_eq!(request.context.caller_scope, "seller-1");
    }

    let spoofed = schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>)
        .execute(
            "mutation { requestLotImageUpload(input: {contentType:\"image/png\", contentLength:42, checksumSha256:\"digest\", principalId:\"attacker\"}, idempotencyKey:\"key\") { imageId } }",
        )
        .await;
    assert!(!spoofed.errors.is_empty());
    assert_eq!(api.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn mutation_parsing_and_error_extensions_are_stable() {
    let api = Arc::new(MockApi::default());
    let request = Request::new(
        "mutation { listLot(input: {title:\"Lot\", images:[{imageId:\"image-1\", alt:\"front\"}], \
         startPrice:\"100\", currency:\"USD\", bidStep:\"10\", scheduledEnd:\"2023-11-14T22:14:20Z\", \
         inactivitySeconds:30, hardDeadline:\"2023-11-14T23:14:20Z\"}, idempotencyKey:\"list-1\") { id } }",
    )
    .data(verified());
    let response = schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>)
        .execute(request)
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    {
        let listings = api.listings.lock().unwrap();
        let listing = &listings[0];
        assert_eq!(
            listing.context.principal.as_ref().unwrap().principal_id,
            "seller-1"
        );
        assert_eq!(listing.bid_step_minor, 10);
        assert_eq!(listing.closing_policy.inactivity_millis, Some(30_000));
    }

    *api.error.lock().unwrap() = Some(ApplicationError::DependencyUnavailable {
        dependency: "private.database.endpoint",
    });
    let failed = schema(api).execute("{ lot(id: \"lot-1\") { id } }").await;
    let error = &failed.errors[0];
    assert_eq!(
        error.extensions.as_ref().unwrap().get("code"),
        Some(&value!("dependency_unavailable"))
    );
    assert_eq!(
        error.extensions.as_ref().unwrap().get("retryable"),
        Some(&value!(true))
    );
    assert!(!error.message.contains("private.database"));
}

#[tokio::test]
async fn list_lot_reuses_server_generated_id_for_exact_retry() {
    let api = Arc::new(MockApi::default());
    let document = "mutation { listLot(input: {title:\"Lot\", images:[{imageId:\"image-1\", alt:\"front\"}], \
         startPrice:\"100\", currency:\"USD\", scheduledEnd:\"2023-11-14T22:14:20Z\"}, \
         idempotencyKey:\"stable-list-key\") { id } }";

    let first = schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>)
        .execute(Request::new(document).data(verified()))
        .await;
    let mut later = verified();
    later.now_millis += 5_000;
    let retry = schema(Arc::clone(&api) as Arc<dyn CatalogGraphqlApi>)
        .execute(Request::new(document).data(later))
        .await;

    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert!(retry.errors.is_empty(), "{:?}", retry.errors);
    let listings = api.listings.lock().unwrap();
    assert_eq!(listings.len(), 2);
    assert_eq!(listings[0].lot_id, listings[1].lot_id);
    assert_ne!(
        listings[0].occurred_at_millis,
        listings[1].occurred_at_millis
    );
}

#[test]
fn executable_schema_contains_owned_federation_contract_without_private_fields() {
    let sdl = schema(Arc::new(MockApi::default()))
        .sdl_with_options(async_graphql::SDLExportOptions::new().federation());
    for field in [
        "lot(",
        "lots(",
        "requestLotImageUpload",
        "completeLotImageUpload",
        "listLot",
        "updateLot",
        "type Lot @key(fields: \"id\")",
    ] {
        assert!(sdl.contains(field), "missing {field} in:\n{sdl}");
    }
    for private in [
        "objectKey",
        "authzRevision",
        "receipt",
        "consistencyToken",
        "principalId",
    ] {
        assert!(!sdl.contains(private), "private field leaked: {private}");
    }
}
