import { defineApi, createClient, del, empty, get, json, post } from "@askrjs/fetch";
import type { ClientOptions } from "@askrjs/fetch";
import type { CollectionCreateRequest, ConsistencyCheckRequest, CreateCollectionResponse, CreateDocumentResponse, CreateIndexRequest, DeleteDocumentResponse, DocumentPayload, Error, ExportManifestRequest, Health, ProjectionCheckReport, ProjectionConsistencyReports, ProjectionManifest, QueryExecuteRequest, QueryExplainRequest, QueryExplainResponse, QueryResult, QuerySchemaResponse, QueryValidateRequest, QueryValidateResponse, SearchRequest, Session, VectorIndexResponse } from "./schemas";
import type { CreateDocumentPath, CreateIndexPath, CreateProjectionVerificationManifestPath, DeleteDocumentPath, ExportProjectionManifestPath, GetDocumentPath, VectorSearchPath } from "./operations";

export const api = defineApi({
  listAdminCatalog: get("/api/v1/admin/catalog")
    .returns(json<QuerySchemaResponse>())
    .errors({ "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  compareProjectionConsistency: post("/api/v1/admin/projection-consistency-checks")
    .body(json<ConsistencyCheckRequest>())
    .returns(json<ProjectionCheckReport>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  getProjectionConsistencyReports: get("/api/v1/admin/projection-consistency-reports")
    .returns(json<ProjectionConsistencyReports>())
    .errors({ "401": json<Error>(), "403": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createProjectionConsistencyReport: post("/api/v1/admin/projection-consistency-reports")
    .body(json<ConsistencyCheckRequest>())
    .returns(json<ProjectionCheckReport>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  exportProjectionManifest: post("/api/v1/admin/projections/{projection}/verification-manifest")
    .params<ExportProjectionManifestPath>({ "projection": { style: "simple", explode: false } })
    .body(json<ExportManifestRequest>())
    .returns(json<ProjectionManifest>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createProjectionVerificationManifest: post("/api/v1/admin/projections/{projection}/verification-manifests")
    .params<CreateProjectionVerificationManifestPath>({ "projection": { style: "simple", explode: false } })
    .body(json<ExportManifestRequest>())
    .returns(json<ProjectionManifest>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createAdminQueryExecution: post("/api/v1/admin/query-executions")
    .body(json<QueryExecuteRequest>())
    .returns(json<QueryResult>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createAdminQueryExplanation: post("/api/v1/admin/query-explanations")
    .body(json<QueryExplainRequest>())
    .returns(json<QueryExplainResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createAdminQueryValidation: post("/api/v1/admin/query-validations")
    .body(json<QueryValidateRequest>())
    .returns(json<QueryValidateResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  executeAdminQuery: post("/api/v1/admin/query/execute")
    .body(json<QueryExecuteRequest>())
    .returns(json<QueryResult>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  explainAdminQuery: post("/api/v1/admin/query/explain")
    .body(json<QueryExplainRequest>())
    .returns(json<QueryExplainResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  getAdminQuerySchema: get("/api/v1/admin/query/schema")
    .returns(json<QuerySchemaResponse>())
    .errors({ "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  validateAdminQuery: post("/api/v1/admin/query/validate")
    .body(json<QueryValidateRequest>())
    .returns(json<QueryValidateResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "405": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  loginRestSession: post("/api/v1/auth/login")
    .body(json<{
  "username": string;
  "password": string;
  "database"?: string;
}>())
    .returns(json<Session>())
    .errors({ "401": json<Error>(), "408": json<Error>(), "413": json<Error>(), "415": json<Error>() }),
  logoutRestSession: post("/api/v1/auth/logout")
    .returns(empty())
    .security([{"cassieAuth":[]}]),
  getRestSession: get("/api/v1/auth/session")
    .returns(json<Session>())
    .errors({ "401": json<Error>() })
    .security([{"cassieAuth":[]}]),
  listCollections: get("/api/v1/collections")
    .returns(json<Array<string>>())
    .errors({ "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createCollection: post("/api/v1/collections")
    .body(json<CollectionCreateRequest>())
    .returns(json<CreateCollectionResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "409": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createDocument: post("/api/v1/collections/{collection}/documents")
    .params<CreateDocumentPath>({ "collection": { style: "simple", explode: false } })
    .body(json<DocumentPayload>())
    .returns(json<CreateDocumentResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  getDocument: get("/api/v1/collections/{collection}/documents/{id}")
    .params<GetDocumentPath>({ "collection": { style: "simple", explode: false }, "id": { style: "simple", explode: false } })
    .returns(json<DocumentPayload>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  deleteDocument: del("/api/v1/collections/{collection}/documents/{id}")
    .params<DeleteDocumentPath>({ "collection": { style: "simple", explode: false }, "id": { style: "simple", explode: false } })
    .returns(json<DeleteDocumentResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  createIndex: post("/api/v1/collections/{collection}/indexes")
    .params<CreateIndexPath>({ "collection": { style: "simple", explode: false } })
    .body(json<CreateIndexRequest>())
    .returns(json<VectorIndexResponse>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "409": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  vectorSearch: post("/api/v1/collections/{collection}/search")
    .params<VectorSearchPath>({ "collection": { style: "simple", explode: false } })
    .body(json<SearchRequest>())
    .returns(json<QueryResult>())
    .errors({ "400": json<Error>(), "401": json<Error>(), "403": json<Error>(), "404": json<Error>(), "500": json<Error>() })
    .security([{"cassieAuth":[]}]),
  getHealth: get("/health")
    .returns(json<Health>())
    .errors({ "500": json<Error>() }),
  getLiveness: get("/liveness")
    .returns(json<Health>())
    .errors({ "500": json<Error>() }),
  getMetrics: get("/metrics")
    .returns(json<{

}>())
    .errors({ "500": json<Error>() }),
  getTargetz: get("/targetz")
    .returns(json<Health>())
    .errors({ "500": json<Error>() }),
}, {
  "servers": [
    "/"
  ],
  "securitySchemes": {
    "cassieAuth": {
      "type": "apiKey",
      "in": "cookie",
      "name": "cassie_session",
      "description": "Server-owned opaque session cookie. Obtain it from the login endpoint; the token value is never persisted in plaintext."
    }
  }
});

export const createApiClient = (options?: ClientOptions) => createClient(api, options);
