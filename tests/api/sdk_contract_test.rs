use tempfile::tempdir;
use trident::api::service::{PrimitiveFieldBytes, PrimitiveFieldId, RequestContext};
use trident::identity::{Cid, Eid};
use trident::sdk::{
    GraphQlTridentClient, GrpcTridentClient, LocalPageTridentClient, LocalTridentClient,
    RestTridentClient, SdkTransport,
};
use trident::store::StorageEngine;

#[test]
fn local_sdk_executes_primitive_record_flow() {
    let dir = tempdir().unwrap();
    let engine = StorageEngine::open(dir.path(), 1024 * 1024).unwrap();
    let client = LocalTridentClient::new(engine);

    let put = client
        .put_record(
            RequestContext::new("sdk-local-put"),
            b"sdk-value".to_vec(),
            Vec::new(),
        )
        .unwrap();
    let get = client
        .get_record(RequestContext::new("sdk-local-get"), put.record_id)
        .unwrap();

    assert_eq!(get.value, Some(b"sdk-value".to_vec()));
    assert!(
        client
            .delete_record(RequestContext::new("sdk-local-delete"), put.record_id)
            .unwrap()
            .deleted
    );
}

#[test]
fn local_page_sdk_executes_cid_eid_rid_flow() {
    let dir = tempdir().unwrap();
    let client = LocalPageTridentClient::open(dir.path());

    let put = client
        .put_page_record(
            RequestContext::new("sdk-page-put"),
            Cid(1),
            Eid(2),
            vec![PrimitiveFieldBytes {
                field: PrimitiveFieldId::Fixed(1),
                bytes: b"page-sdk-value".to_vec(),
            }],
        )
        .unwrap();
    let get = client
        .get_page_record(RequestContext::new("sdk-page-get"), Cid(1), Eid(2), put.rid)
        .unwrap();

    assert_eq!(get.body, Some(b"page-sdk-value".to_vec()));
    assert!(
        client
            .delete_page_record(
                RequestContext::new("sdk-page-delete"),
                Cid(1),
                Eid(2),
                put.rid
            )
            .unwrap()
            .deleted
    );
}

#[test]
fn remote_sdk_clients_preserve_request_ids_and_transport() {
    let rest = RestTridentClient::new("http://127.0.0.1:8080")
        .put_record_request(RequestContext::new("rest-1"), b"body".to_vec());
    let grpc = GrpcTridentClient::new("http://127.0.0.1:50051")
        .put_record_request(RequestContext::new("grpc-1"), b"body".to_vec());
    let graphql = GraphQlTridentClient::new("http://127.0.0.1:8080/graphql")
        .put_record_request(RequestContext::new("graphql-1"), b"body".to_vec());

    assert_eq!(rest.transport, SdkTransport::Rest);
    assert_eq!(rest.request_id, "rest-1");
    assert_eq!(grpc.transport, SdkTransport::Grpc);
    assert_eq!(grpc.request_id, "grpc-1");
    assert_eq!(graphql.transport, SdkTransport::GraphQl);
    assert_eq!(graphql.request_id, "graphql-1");
}
