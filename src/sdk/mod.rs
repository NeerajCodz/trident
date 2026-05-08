use crate::api::service::{
    DeleteRecordRequest, DeleteRecordResponse, GetRecordRequest, GetRecordResponse,
    PagePrimitiveStorageService, PrimitiveFieldBytes, PrimitiveStorageService, PutRecordRequest,
    PutRecordResponse, RequestContext, StatsRequest, StatsResponse,
};
use crate::errors::Result;
use crate::identity::{Cid, Eid, Rid};
use crate::store::{IndexInsert, RecordId, StorageEngine};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SdkTransport {
    Local,
    Rest,
    Grpc,
    GraphQl,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteStorageRequest {
    pub transport: SdkTransport,
    pub request_id: String,
    pub operation: String,
    pub path: String,
    pub body: Vec<u8>,
}

pub struct LocalTridentClient {
    service: PrimitiveStorageService,
}

pub struct LocalPageTridentClient {
    service: PagePrimitiveStorageService,
}

impl LocalTridentClient {
    pub fn new(engine: StorageEngine) -> Self {
        Self {
            service: PrimitiveStorageService::new(engine),
        }
    }

    pub fn put_record(
        &self,
        context: RequestContext,
        value: Vec<u8>,
        indexes: Vec<IndexInsert>,
    ) -> Result<PutRecordResponse> {
        self.service.put_record(PutRecordRequest {
            context,
            value,
            indexes,
        })
    }

    pub fn get_record(
        &self,
        context: RequestContext,
        record_id: RecordId,
    ) -> Result<GetRecordResponse> {
        self.service
            .get_record(GetRecordRequest { context, record_id })
    }

    pub fn delete_record(
        &self,
        context: RequestContext,
        record_id: RecordId,
    ) -> Result<DeleteRecordResponse> {
        self.service
            .delete_record(DeleteRecordRequest { context, record_id })
    }

    pub fn stats(&self, context: RequestContext) -> Result<StatsResponse> {
        self.service.stats(StatsRequest { context })
    }
}

impl LocalPageTridentClient {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self {
            service: PagePrimitiveStorageService::open(root),
        }
    }

    pub fn put_page_record(
        &self,
        context: RequestContext,
        cid: Cid,
        eid: Eid,
        fields: Vec<PrimitiveFieldBytes>,
    ) -> Result<crate::api::service::PutPageRecordResponse> {
        self.service
            .put_page_record(crate::api::service::PutPageRecordRequest {
                context,
                cid,
                eid,
                fields,
            })
    }

    pub fn get_page_record(
        &self,
        context: RequestContext,
        cid: Cid,
        eid: Eid,
        rid: Rid,
    ) -> Result<crate::api::service::GetPageRecordResponse> {
        self.service
            .get_page_record(crate::api::service::GetPageRecordRequest {
                context,
                cid,
                eid,
                rid,
            })
    }

    pub fn delete_page_record(
        &self,
        context: RequestContext,
        cid: Cid,
        eid: Eid,
        rid: Rid,
    ) -> Result<crate::api::service::DeletePageRecordResponse> {
        self.service
            .delete_page_record(crate::api::service::DeletePageRecordRequest {
                context,
                cid,
                eid,
                rid,
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestTridentClient {
    endpoint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrpcTridentClient {
    endpoint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphQlTridentClient {
    endpoint: String,
}

impl RestTridentClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn put_record_request(
        &self,
        context: RequestContext,
        body: Vec<u8>,
    ) -> RemoteStorageRequest {
        remote_request(
            SdkTransport::Rest,
            context,
            "put_record",
            format!("{}/v1/records", self.endpoint),
            body,
        )
    }
}

impl GrpcTridentClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn put_record_request(
        &self,
        context: RequestContext,
        body: Vec<u8>,
    ) -> RemoteStorageRequest {
        remote_request(
            SdkTransport::Grpc,
            context,
            "trident.storage.v1.RecordService/PutRecord",
            self.endpoint.clone(),
            body,
        )
    }
}

impl GraphQlTridentClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    pub fn put_record_request(
        &self,
        context: RequestContext,
        body: Vec<u8>,
    ) -> RemoteStorageRequest {
        remote_request(
            SdkTransport::GraphQl,
            context,
            "mutation.putRecord",
            self.endpoint.clone(),
            body,
        )
    }
}

fn remote_request(
    transport: SdkTransport,
    context: RequestContext,
    operation: impl Into<String>,
    path: impl Into<String>,
    body: Vec<u8>,
) -> RemoteStorageRequest {
    RemoteStorageRequest {
        transport,
        request_id: context.request_id,
        operation: operation.into(),
        path: path.into(),
        body,
    }
}
