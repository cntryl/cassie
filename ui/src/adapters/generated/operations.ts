import type { CollectionCreateRequest, ConsistencyCheckRequest, CreateCollectionResponse, CreateDocumentResponse, CreateIndexRequest, DeleteDocumentResponse, DocumentPayload, Error, ExportManifestRequest, Health, ProjectionCheckReport, ProjectionConsistencyReports, ProjectionManifest, QueryExecuteRequest, QueryExplainRequest, QueryExplainResponse, QueryResult, QuerySchemaResponse, QueryValidateRequest, QueryValidateResponse, SearchRequest, Session, VectorIndexResponse } from "./schemas";

export type ListAdminCatalogResponse200 = QuerySchemaResponse;

export type ListAdminCatalogError_401 = Error;

export type ListAdminCatalogError_403 = Error;

export type ListAdminCatalogError_405 = Error;

export type ListAdminCatalogError_500 = Error;

export type CompareProjectionConsistencyBody = ConsistencyCheckRequest;

export type CompareProjectionConsistencyResponse200 = ProjectionCheckReport;

export type CompareProjectionConsistencyError_400 = Error;

export type CompareProjectionConsistencyError_401 = Error;

export type CompareProjectionConsistencyError_403 = Error;

export type CompareProjectionConsistencyError_500 = Error;

export type GetProjectionConsistencyReportsResponse200 = ProjectionConsistencyReports;

export type GetProjectionConsistencyReportsError_401 = Error;

export type GetProjectionConsistencyReportsError_403 = Error;

export type GetProjectionConsistencyReportsError_500 = Error;

export type CreateProjectionConsistencyReportBody = ConsistencyCheckRequest;

export type CreateProjectionConsistencyReportResponse200 = ProjectionCheckReport;

export type CreateProjectionConsistencyReportError_400 = Error;

export type CreateProjectionConsistencyReportError_401 = Error;

export type CreateProjectionConsistencyReportError_403 = Error;

export type CreateProjectionConsistencyReportError_500 = Error;

export type ExportProjectionManifestPath = {
  "projection": string;
};

export type ExportProjectionManifestBody = ExportManifestRequest;

export type ExportProjectionManifestResponse200 = ProjectionManifest;

export type ExportProjectionManifestError_400 = Error;

export type ExportProjectionManifestError_401 = Error;

export type ExportProjectionManifestError_403 = Error;

export type ExportProjectionManifestError_404 = Error;

export type ExportProjectionManifestError_500 = Error;

export type CreateProjectionVerificationManifestPath = {
  "projection": string;
};

export type CreateProjectionVerificationManifestBody = ExportManifestRequest;

export type CreateProjectionVerificationManifestResponse200 = ProjectionManifest;

export type CreateProjectionVerificationManifestError_400 = Error;

export type CreateProjectionVerificationManifestError_401 = Error;

export type CreateProjectionVerificationManifestError_403 = Error;

export type CreateProjectionVerificationManifestError_404 = Error;

export type CreateProjectionVerificationManifestError_500 = Error;

export type CreateAdminQueryExecutionBody = QueryExecuteRequest;

export type CreateAdminQueryExecutionResponse200 = QueryResult;

export type CreateAdminQueryExecutionError_400 = Error;

export type CreateAdminQueryExecutionError_401 = Error;

export type CreateAdminQueryExecutionError_403 = Error;

export type CreateAdminQueryExecutionError_405 = Error;

export type CreateAdminQueryExecutionError_500 = Error;

export type CreateAdminQueryExplanationBody = QueryExplainRequest;

export type CreateAdminQueryExplanationResponse200 = QueryExplainResponse;

export type CreateAdminQueryExplanationError_400 = Error;

export type CreateAdminQueryExplanationError_401 = Error;

export type CreateAdminQueryExplanationError_403 = Error;

export type CreateAdminQueryExplanationError_405 = Error;

export type CreateAdminQueryExplanationError_500 = Error;

export type CreateAdminQueryValidationBody = QueryValidateRequest;

export type CreateAdminQueryValidationResponse200 = QueryValidateResponse;

export type CreateAdminQueryValidationError_400 = Error;

export type CreateAdminQueryValidationError_401 = Error;

export type CreateAdminQueryValidationError_403 = Error;

export type CreateAdminQueryValidationError_405 = Error;

export type CreateAdminQueryValidationError_500 = Error;

export type ExecuteAdminQueryBody = QueryExecuteRequest;

export type ExecuteAdminQueryResponse200 = QueryResult;

export type ExecuteAdminQueryError_400 = Error;

export type ExecuteAdminQueryError_401 = Error;

export type ExecuteAdminQueryError_403 = Error;

export type ExecuteAdminQueryError_405 = Error;

export type ExecuteAdminQueryError_500 = Error;

export type ExplainAdminQueryBody = QueryExplainRequest;

export type ExplainAdminQueryResponse200 = QueryExplainResponse;

export type ExplainAdminQueryError_400 = Error;

export type ExplainAdminQueryError_401 = Error;

export type ExplainAdminQueryError_403 = Error;

export type ExplainAdminQueryError_405 = Error;

export type ExplainAdminQueryError_500 = Error;

export type GetAdminQuerySchemaResponse200 = QuerySchemaResponse;

export type GetAdminQuerySchemaError_401 = Error;

export type GetAdminQuerySchemaError_403 = Error;

export type GetAdminQuerySchemaError_405 = Error;

export type GetAdminQuerySchemaError_500 = Error;

export type ValidateAdminQueryBody = QueryValidateRequest;

export type ValidateAdminQueryResponse200 = QueryValidateResponse;

export type ValidateAdminQueryError_400 = Error;

export type ValidateAdminQueryError_401 = Error;

export type ValidateAdminQueryError_403 = Error;

export type ValidateAdminQueryError_405 = Error;

export type ValidateAdminQueryError_500 = Error;

export type LoginRestSessionBody = {
  "username": string;
  "password": string;
  "database"?: string;
};

export type LoginRestSessionResponse200 = Session;

export type LoginRestSessionError_401 = Error;

export type LoginRestSessionError_408 = Error;

export type LoginRestSessionError_413 = Error;

export type LoginRestSessionError_415 = Error;

export type LogoutRestSessionResponse200 = undefined;

export type GetRestSessionResponse200 = Session;

export type GetRestSessionError_401 = Error;

export type ListCollectionsResponse200 = Array<string>;

export type ListCollectionsError_500 = Error;

export type CreateCollectionBody = CollectionCreateRequest;

export type CreateCollectionResponse200 = CreateCollectionResponse;

export type CreateCollectionError_400 = Error;

export type CreateCollectionError_401 = Error;

export type CreateCollectionError_403 = Error;

export type CreateCollectionError_409 = Error;

export type CreateCollectionError_500 = Error;

export type CreateDocumentPath = {
  "collection": string;
};

export type CreateDocumentBody = DocumentPayload;

export type CreateDocumentResponse200 = CreateDocumentResponse;

export type CreateDocumentError_400 = Error;

export type CreateDocumentError_401 = Error;

export type CreateDocumentError_403 = Error;

export type CreateDocumentError_404 = Error;

export type CreateDocumentError_500 = Error;

export type GetDocumentPath = {
  "collection": string;
  "id": string;
};

export type GetDocumentResponse200 = DocumentPayload;

export type GetDocumentError_400 = Error;

export type GetDocumentError_401 = Error;

export type GetDocumentError_403 = Error;

export type GetDocumentError_404 = Error;

export type GetDocumentError_500 = Error;

export type DeleteDocumentPath = {
  "collection": string;
  "id": string;
};

export type DeleteDocumentResponse200 = DeleteDocumentResponse;

export type DeleteDocumentError_400 = Error;

export type DeleteDocumentError_401 = Error;

export type DeleteDocumentError_403 = Error;

export type DeleteDocumentError_404 = Error;

export type DeleteDocumentError_500 = Error;

export type CreateIndexPath = {
  "collection": string;
};

export type CreateIndexBody = CreateIndexRequest;

export type CreateIndexResponse200 = VectorIndexResponse;

export type CreateIndexError_400 = Error;

export type CreateIndexError_401 = Error;

export type CreateIndexError_403 = Error;

export type CreateIndexError_404 = Error;

export type CreateIndexError_409 = Error;

export type CreateIndexError_500 = Error;

export type VectorSearchPath = {
  "collection": string;
};

export type VectorSearchBody = SearchRequest;

export type VectorSearchResponse200 = QueryResult;

export type VectorSearchError_400 = Error;

export type VectorSearchError_401 = Error;

export type VectorSearchError_403 = Error;

export type VectorSearchError_404 = Error;

export type VectorSearchError_500 = Error;

export type GetHealthResponse200 = Health;

export type GetHealthError_500 = Error;

export type GetLivenessResponse200 = Health;

export type GetLivenessError_500 = Error;

export type GetMetricsResponse200 = {

};

export type GetMetricsError_500 = Error;

export type GetTargetzResponse200 = Health;

export type GetTargetzError_500 = Error;
