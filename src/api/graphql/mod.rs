pub const STORAGE_GRAPHQL_SCHEMA: &str = r#"
schema {
  query: Query
  mutation: Mutation
}

type Query {
  health: Health!
  stats(context: RequestContextInput!): StorageStats!
  record(context: RequestContextInput!, recordId: ID!): Record
  snapshot(context: RequestContextInput!): Snapshot!
}

type Mutation {
  putRecord(context: RequestContextInput!, value: Bytes!, indexes: [IndexMutationInput!]!): PutRecordPayload!
  deleteRecord(context: RequestContextInput!, recordId: ID!): DeleteRecordPayload!
  flush(context: RequestContextInput!): OperationPayload!
  compact(context: RequestContextInput!): CompactPayload!
}

input RequestContextInput {
  requestId: String!
  tenantId: String
  traceId: String
}

input IndexMutationInput {
  indexType: String!
  key: Bytes!
}

scalar Bytes

type Health { status: String! }
type Record { recordId: ID!, value: Bytes! }
type Snapshot { sequence: String! }
type StorageStats { liveRecords: String!, liveBytes: String!, walBytesWritten: String! }
type PutRecordPayload { recordId: ID! }
type DeleteRecordPayload { ok: Boolean! }
type OperationPayload { ok: Boolean! }
type CompactPayload { recordsRetained: String!, recordsDropped: String!, bytesRewritten: String! }
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphQlPrimitive {
    Health,
    Stats,
    Record,
    Snapshot,
    PutRecord,
    DeleteRecord,
    Flush,
    Compact,
}

impl GraphQlPrimitive {
    pub fn field_name(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Stats => "stats",
            Self::Record => "record",
            Self::Snapshot => "snapshot",
            Self::PutRecord => "putRecord",
            Self::DeleteRecord => "deleteRecord",
            Self::Flush => "flush",
            Self::Compact => "compact",
        }
    }
}
