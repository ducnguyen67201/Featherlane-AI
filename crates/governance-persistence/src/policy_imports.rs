use async_trait::async_trait;
use governance_application::{
    ApplicationError, PolicyImportRepository, PolicyPackRepository, VerifySourceCommand,
};
use governance_domain::{
    CandidateReview, DocumentFormat, ExtractionBatch, ParsedDocument, PolicyBundle,
    PolicyCandidate, PolicyCandidateId, PolicyCandidateReviewRecord, PolicyImport,
    PolicyImportCoverage, PolicyImportId, PolicyImportStatus, PolicyPack, PolicyPackId,
    SourceVerificationStatus,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, SqlErr, TransactionTrait,
    sea_query::{Condition, Expr},
};
use time::OffsetDateTime;

use crate::{
    SeaOrmPolicyPackRepository,
    entities::{policy_candidate_reviews, policy_candidates, policy_imports},
    enum_string, persist_bundle, repository_error, serialization_error,
};

#[derive(Clone, Debug)]
pub struct SeaOrmPolicyImportRepository {
    database: DatabaseConnection,
}

impl SeaOrmPolicyImportRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }

    async fn require_import(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
    ) -> Result<policy_imports::Model, ApplicationError> {
        policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }
}

#[async_trait]
impl PolicyImportRepository for SeaOrmPolicyImportRepository {
    async fn create(&self, import: &PolicyImport) -> Result<PolicyImport, ApplicationError> {
        super::ensure_organization(&self.database, import.organization_id).await?;
        let active = import_active_model(import)?;
        if let Err(error) = active.insert(&self.database).await {
            if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                if let Some(idempotency_key) = import.idempotency_key.as_deref()
                    && policy_imports::Entity::find()
                        .filter(policy_imports::Column::OrganizationId.eq(import.organization_id.0))
                        .filter(policy_imports::Column::IdempotencyKey.eq(idempotency_key))
                        .one(&self.database)
                        .await
                        .map_err(repository_error)?
                        .is_some()
                {
                    return Err(ApplicationError::Conflict(
                        "Idempotency-Key was already used".to_owned(),
                    ));
                }
                if policy_imports::Entity::find()
                    .filter(policy_imports::Column::PolicySourceId.eq(import.policy_source_id.0))
                    .filter(
                        policy_imports::Column::Revision
                            .eq(i32::try_from(import.revision).unwrap_or(i32::MAX)),
                    )
                    .one(&self.database)
                    .await
                    .map_err(repository_error)?
                    .is_some()
                {
                    return Err(ApplicationError::Conflict(
                        "a newer revision of this policy source already exists".to_owned(),
                    ));
                }
            }
            return Err(repository_error(error));
        }
        Ok(import.clone())
    }

    async fn get(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
    ) -> Result<Option<PolicyImport>, ApplicationError> {
        policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(import_from_model)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: governance_domain::OrganizationId,
        limit: u64,
        status: Option<PolicyImportStatus>,
        cursor: Option<PolicyImportId>,
    ) -> Result<Vec<PolicyImport>, ApplicationError> {
        let cursor = if let Some(cursor) = cursor {
            Some(
                policy_imports::Entity::find()
                    .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
                    .filter(policy_imports::Column::Id.eq(cursor.0))
                    .one(&self.database)
                    .await
                    .map_err(repository_error)?
                    .ok_or_else(|| ApplicationError::NotFound(cursor.to_string()))?,
            )
        } else {
            None
        };
        let mut query = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0));
        if let Some(status) = status {
            query = query.filter(policy_imports::Column::Status.eq(enum_string(status)?));
        }
        if let Some(cursor) = cursor {
            query = query.filter(
                Condition::any()
                    .add(policy_imports::Column::CreatedAt.lt(cursor.created_at))
                    .add(
                        Condition::all()
                            .add(policy_imports::Column::CreatedAt.eq(cursor.created_at))
                            .add(policy_imports::Column::Id.lt(cursor.id)),
                    ),
            );
        }
        query
            .order_by_desc(policy_imports::Column::CreatedAt)
            .order_by_desc(policy_imports::Column::Id)
            .limit(limit.min(100))
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(import_from_model)
            .collect()
    }

    async fn transition(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
        expected: PolicyImportStatus,
        next: PolicyImportStatus,
    ) -> Result<PolicyImport, ApplicationError> {
        if !expected.can_transition_to(next) {
            return Err(ApplicationError::InvalidRequest(format!(
                "invalid policy import transition from {expected:?} to {next:?}"
            )));
        }
        let result = policy_imports::Entity::update_many()
            .col_expr(
                policy_imports::Column::Status,
                Expr::value(enum_string(next)?),
            )
            .col_expr(
                policy_imports::Column::UpdatedAt,
                Expr::value(OffsetDateTime::now_utc()),
            )
            .col_expr(
                policy_imports::Column::FailureCode,
                Expr::value(None::<String>),
            )
            .col_expr(
                policy_imports::Column::FailureDetail,
                Expr::value(None::<String>),
            )
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(id.0))
            .filter(policy_imports::Column::Status.eq(enum_string(expected)?))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        if result.rows_affected != 1 {
            return Err(ApplicationError::Conflict(format!(
                "policy import {id} is no longer in {expected:?}"
            )));
        }
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn mark_parsed(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
        normalized_object_key: &str,
        document: &ParsedDocument,
        coverage: &PolicyImportCoverage,
    ) -> Result<PolicyImport, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        if model.status != enum_string(PolicyImportStatus::Parsing)? {
            return Err(ApplicationError::Conflict(
                "policy import is not in parsing state".to_owned(),
            ));
        }
        let page_count = document.segments.iter().filter_map(|item| item.page).max();
        let mut active: policy_imports::ActiveModel = model.into();
        active.status = Set(enum_string(PolicyImportStatus::Extracting)?);
        active.normalized_object_key = Set(Some(normalized_object_key.to_owned()));
        active.parser_kind = Set(Some(
            match document.format {
                DocumentFormat::Pdf => "pdf_extract",
                DocumentFormat::Docx => "docx_rs",
                DocumentFormat::PlainText => "plain_text",
            }
            .to_owned(),
        ));
        active.parser_version = Set(Some(document.parser_version.clone()));
        active.page_count = Set(page_count.map(|value| i32::try_from(value).unwrap_or(i32::MAX)));
        active.coverage_payload = Set(serde_json::to_value(coverage).map_err(serialization_error)?);
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn replace_unreviewed_candidates(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
        candidates: &[PolicyCandidate],
        batch: &ExtractionBatch,
    ) -> Result<PolicyImport, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let import = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))?;
        if import.status != enum_string(PolicyImportStatus::Extracting)? {
            return Err(ApplicationError::Conflict(
                "policy import is not in extracting state".to_owned(),
            ));
        }
        let reviewed = policy_candidates::Entity::find()
            .filter(policy_candidates::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_candidates::Column::PolicyImportId.eq(id.0))
            .filter(policy_candidates::Column::ReviewPayload.is_not_null())
            .one(&transaction)
            .await
            .map_err(repository_error)?;
        if reviewed.is_some() {
            return Err(ApplicationError::Conflict(
                "reviewed candidates cannot be replaced by a worker retry".to_owned(),
            ));
        }
        policy_candidates::Entity::delete_many()
            .filter(policy_candidates::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_candidates::Column::PolicyImportId.eq(id.0))
            .exec(&transaction)
            .await
            .map_err(repository_error)?;
        for candidate in candidates {
            candidate_active_model(candidate)?
                .insert(&transaction)
                .await
                .map_err(repository_error)?;
        }
        let mut active: policy_imports::ActiveModel = import.into();
        let extraction_failed = !batch.coverage.failed_chunks.is_empty();
        active.status = Set(enum_string(if extraction_failed {
            PolicyImportStatus::FailedRetryable
        } else {
            PolicyImportStatus::ReviewRequired
        })?);
        active.model_provider = Set(Some(batch.provider.clone()));
        active.model_name = Set(Some(batch.model.clone()));
        active.prompt_version = Set(Some(batch.prompt_version.clone()));
        active.coverage_payload =
            Set(serde_json::to_value(&batch.coverage).map_err(serialization_error)?);
        active.candidate_count = Set(i32::try_from(candidates.len()).unwrap_or(i32::MAX));
        active.failure_code = Set(extraction_failed.then(|| "extraction_chunks_failed".to_owned()));
        active.failure_detail = Set(extraction_failed.then(|| {
            format!(
                "{} extraction chunks failed after bounded retries",
                batch.coverage.failed_chunks.len()
            )
        }));
        active.updated_at = Set(OffsetDateTime::now_utc());
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn list_candidates(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
    ) -> Result<Vec<PolicyCandidate>, ApplicationError> {
        policy_candidates::Entity::find()
            .filter(policy_candidates::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_candidates::Column::PolicyImportId.eq(id.0))
            .order_by_asc(policy_candidates::Column::Position)
            .all(&self.database)
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(candidate_from_model)
            .collect()
    }

    async fn get_candidate(
        &self,
        organization_id: governance_domain::OrganizationId,
        import_id: PolicyImportId,
        candidate_id: PolicyCandidateId,
    ) -> Result<Option<PolicyCandidate>, ApplicationError> {
        policy_candidates::Entity::find()
            .filter(policy_candidates::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_candidates::Column::PolicyImportId.eq(import_id.0))
            .filter(policy_candidates::Column::Id.eq(candidate_id.0))
            .one(&self.database)
            .await
            .map_err(repository_error)?
            .map(candidate_from_model)
            .transpose()
    }

    async fn save_candidate_review(
        &self,
        candidate: &PolicyCandidate,
        review: &PolicyCandidateReviewRecord,
        expected_updated_at: Option<OffsetDateTime>,
    ) -> Result<PolicyCandidate, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let import = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(candidate.organization_id.0))
            .filter(policy_imports::Column::Id.eq(candidate.policy_import_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(candidate.policy_import_id.to_string()))?;
        if import.status == enum_string(PolicyImportStatus::Compiled)? {
            return Err(ApplicationError::Conflict(
                "compiled imports are immutable".to_owned(),
            ));
        }
        let model = policy_candidates::Entity::find()
            .filter(policy_candidates::Column::OrganizationId.eq(candidate.organization_id.0))
            .filter(policy_candidates::Column::PolicyImportId.eq(candidate.policy_import_id.0))
            .filter(policy_candidates::Column::Id.eq(candidate.id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(candidate.id.to_string()))?;
        if expected_updated_at.is_some_and(|expected| expected != model.updated_at) {
            return Err(ApplicationError::Conflict(
                "candidate was updated by another reviewer".to_owned(),
            ));
        }
        let mut active = candidate_active_model(candidate)?;
        active.id = Set(model.id);
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        review_active_model(review)?
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        Ok(candidate.clone())
    }

    async fn add_manual_candidate(
        &self,
        candidate: &PolicyCandidate,
        review: &PolicyCandidateReviewRecord,
    ) -> Result<PolicyCandidate, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let import = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(candidate.organization_id.0))
            .filter(policy_imports::Column::Id.eq(candidate.policy_import_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(candidate.policy_import_id.to_string()))?;
        if !matches!(
            enum_from_string(&import.status)?,
            PolicyImportStatus::ReviewRequired | PolicyImportStatus::ReadyToCompile
        ) {
            return Err(ApplicationError::Conflict(
                "manual candidates can only be added during review".to_owned(),
            ));
        }
        candidate_active_model(candidate)?
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        review_active_model(review)?
            .insert(&transaction)
            .await
            .map_err(repository_error)?;
        policy_imports::Entity::update_many()
            .col_expr(
                policy_imports::Column::CandidateCount,
                sea_orm::ExprTrait::add(Expr::col(policy_imports::Column::CandidateCount), 1),
            )
            .col_expr(
                policy_imports::Column::UpdatedAt,
                Expr::value(OffsetDateTime::now_utc()),
            )
            .filter(policy_imports::Column::OrganizationId.eq(candidate.organization_id.0))
            .filter(policy_imports::Column::Id.eq(candidate.policy_import_id.0))
            .exec(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        Ok(candidate.clone())
    }

    async fn verify_source(
        &self,
        command: &VerifySourceCommand,
        reviewed_at: OffsetDateTime,
    ) -> Result<PolicyImport, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(command.organization_id.0))
            .filter(policy_imports::Column::Id.eq(command.policy_import_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(command.policy_import_id.to_string()))?;
        if model.status == enum_string(PolicyImportStatus::Compiled)? {
            return Err(ApplicationError::Conflict(
                "compiled imports are immutable".to_owned(),
            ));
        }
        if ![
            enum_string(PolicyImportStatus::ReviewRequired)?,
            enum_string(PolicyImportStatus::ReadyToCompile)?,
        ]
        .contains(&model.status)
        {
            return Err(ApplicationError::Conflict(
                "source can only be verified after extraction completes".to_owned(),
            ));
        }
        let mut active: policy_imports::ActiveModel = model.into();
        active.verification_status = Set(enum_string(if command.verified {
            SourceVerificationStatus::Verified
        } else {
            SourceVerificationStatus::Rejected
        })?);
        active.verified_by = Set(Some(command.reviewer_id.clone()));
        active.verified_at = Set(Some(reviewed_at));
        active.verification_notes = Set(Some(command.notes.clone()));
        active.updated_at = Set(reviewed_at);
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        self.get(command.organization_id, command.policy_import_id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(command.policy_import_id.to_string()))
    }

    async fn set_review_state(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
        status: PolicyImportStatus,
    ) -> Result<PolicyImport, ApplicationError> {
        if !matches!(
            status,
            PolicyImportStatus::ReviewRequired | PolicyImportStatus::ReadyToCompile
        ) {
            return Err(ApplicationError::InvalidRequest(
                "review state must be review_required or ready_to_compile".to_owned(),
            ));
        }
        let result = policy_imports::Entity::update_many()
            .col_expr(
                policy_imports::Column::Status,
                Expr::value(enum_string(status)?),
            )
            .col_expr(
                policy_imports::Column::UpdatedAt,
                Expr::value(OffsetDateTime::now_utc()),
            )
            .filter(policy_imports::Column::OrganizationId.eq(organization_id.0))
            .filter(policy_imports::Column::Id.eq(id.0))
            .filter(policy_imports::Column::Status.is_in([
                enum_string(PolicyImportStatus::ReviewRequired)?,
                enum_string(PolicyImportStatus::ReadyToCompile)?,
            ]))
            .exec(&self.database)
            .await
            .map_err(repository_error)?;
        if result.rows_affected != 1 {
            return Err(ApplicationError::Conflict(
                "policy import is not in a reviewable state".to_owned(),
            ));
        }
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn mark_failure(
        &self,
        organization_id: governance_domain::OrganizationId,
        id: PolicyImportId,
        status: PolicyImportStatus,
        code: &str,
        detail: &str,
    ) -> Result<PolicyImport, ApplicationError> {
        if !matches!(
            status,
            PolicyImportStatus::NeedsOcr
                | PolicyImportStatus::FailedRetryable
                | PolicyImportStatus::FailedTerminal
        ) {
            return Err(ApplicationError::InvalidRequest(
                "failure status is invalid".to_owned(),
            ));
        }
        let model = self.require_import(organization_id, id).await?;
        let mut active: policy_imports::ActiveModel = model.into();
        active.status = Set(enum_string(status)?);
        active.failure_code = Set(Some(code.to_owned()));
        active.failure_detail = Set(Some(detail.to_owned()));
        active.updated_at = Set(OffsetDateTime::now_utc());
        if matches!(
            status,
            PolicyImportStatus::NeedsOcr | PolicyImportStatus::FailedTerminal
        ) {
            active.completed_at = Set(Some(OffsetDateTime::now_utc()));
        }
        active
            .update(&self.database)
            .await
            .map_err(repository_error)?;
        self.get(organization_id, id)
            .await?
            .ok_or_else(|| ApplicationError::NotFound(id.to_string()))
    }

    async fn save_compiled_bundle(
        &self,
        import_id: PolicyImportId,
        bundle: &PolicyBundle,
    ) -> Result<PolicyPack, ApplicationError> {
        let transaction = self.database.begin().await.map_err(repository_error)?;
        let model = policy_imports::Entity::find()
            .filter(policy_imports::Column::OrganizationId.eq(bundle.pack.organization_id.0))
            .filter(policy_imports::Column::Id.eq(import_id.0))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(repository_error)?
            .ok_or_else(|| ApplicationError::NotFound(import_id.to_string()))?;
        if let Some(pack_id) = model.compiled_policy_pack_id {
            transaction.commit().await.map_err(repository_error)?;
            return SeaOrmPolicyPackRepository::new(self.database.clone())
                .get(bundle.pack.organization_id, PolicyPackId(pack_id))
                .await?
                .ok_or_else(|| ApplicationError::NotFound(pack_id.to_string()));
        }
        if model.status != enum_string(PolicyImportStatus::ReadyToCompile)? {
            return Err(ApplicationError::Conflict(
                "policy import is not ready to compile".to_owned(),
            ));
        }
        persist_bundle(&transaction, bundle).await?;
        let source_id = bundle.sources.first().ok_or_else(|| {
            ApplicationError::InvalidRequest("compiled bundle requires a source".to_owned())
        })?;
        let mut active: policy_imports::ActiveModel = model.into();
        active.status = Set(enum_string(PolicyImportStatus::Compiled)?);
        active.compiled_source_id = Set(Some(source_id.id.0));
        active.compiled_policy_pack_id = Set(Some(bundle.pack.id.0));
        active.updated_at = Set(OffsetDateTime::now_utc());
        active.completed_at = Set(Some(OffsetDateTime::now_utc()));
        active
            .update(&transaction)
            .await
            .map_err(repository_error)?;
        transaction.commit().await.map_err(repository_error)?;
        Ok(bundle.pack.clone())
    }

    async fn get_compiled_pack(
        &self,
        organization_id: governance_domain::OrganizationId,
        import_id: PolicyImportId,
    ) -> Result<Option<PolicyPack>, ApplicationError> {
        let Some(import) = self.get(organization_id, import_id).await? else {
            return Ok(None);
        };
        let Some(pack_id) = import.compiled_policy_pack_id else {
            return Ok(None);
        };
        SeaOrmPolicyPackRepository::new(self.database.clone())
            .get(organization_id, pack_id)
            .await
    }
}

fn import_active_model(
    import: &PolicyImport,
) -> Result<policy_imports::ActiveModel, ApplicationError> {
    Ok(policy_imports::ActiveModel {
        id: Set(import.id.0),
        organization_id: Set(import.organization_id.0),
        policy_source_id: Set(import.policy_source_id.0),
        revision: Set(i32::try_from(import.revision).unwrap_or(i32::MAX)),
        supersedes_import_id: Set(import.supersedes_import_id.map(|id| id.0)),
        status: Set(enum_string(import.status)?),
        input_kind: Set(enum_string(import.input_kind)?),
        source_type: Set(enum_string(import.source_type)?),
        title: Set(import.title.clone()),
        jurisdiction: Set(import.jurisdiction.clone()),
        effective_from: Set(import.effective_from),
        source_url: Set(import.source_url.clone()),
        original_filename: Set(import.original_filename.clone()),
        declared_mime_type: Set(import.declared_mime_type.clone()),
        detected_mime_type: Set(import.detected_mime_type.clone()),
        byte_length: Set(i64::try_from(import.byte_length).unwrap_or(i64::MAX)),
        content_sha256: Set(import.content_sha256.clone()),
        raw_object_key: Set(import.raw_object_key.clone()),
        normalized_object_key: Set(import.normalized_object_key.clone()),
        parser_kind: Set(import.parser_kind.clone()),
        parser_version: Set(import.parser_version.clone()),
        model_provider: Set(import.model_provider.clone()),
        model_name: Set(import.model_name.clone()),
        prompt_version: Set(import.prompt_version.clone()),
        page_count: Set(import
            .page_count
            .map(|value| i32::try_from(value).unwrap_or(i32::MAX))),
        coverage_payload: Set(serde_json::to_value(&import.coverage).map_err(serialization_error)?),
        candidate_count: Set(i32::try_from(import.candidate_count).unwrap_or(i32::MAX)),
        verification_status: Set(enum_string(import.verification_status)?),
        verified_by: Set(import.verified_by.clone()),
        verified_at: Set(import.verified_at),
        verification_notes: Set(import.verification_notes.clone()),
        failure_code: Set(import.failure_code.clone()),
        failure_detail: Set(import.failure_detail.clone()),
        idempotency_key: Set(import.idempotency_key.clone()),
        compiled_source_id: Set(import.compiled_source_id.map(|id| id.0)),
        compiled_policy_pack_id: Set(import.compiled_policy_pack_id.map(|id| id.0)),
        created_at: Set(import.created_at),
        updated_at: Set(import.updated_at),
        completed_at: Set(import.completed_at),
    })
}

fn import_from_model(model: policy_imports::Model) -> Result<PolicyImport, ApplicationError> {
    Ok(PolicyImport {
        id: PolicyImportId(model.id),
        organization_id: governance_domain::OrganizationId(model.organization_id),
        policy_source_id: governance_domain::PolicySourceId(model.policy_source_id),
        revision: u32::try_from(model.revision).unwrap_or_default(),
        supersedes_import_id: model.supersedes_import_id.map(PolicyImportId),
        status: enum_from_string(&model.status)?,
        input_kind: enum_from_string(&model.input_kind)?,
        source_type: enum_from_string(&model.source_type)?,
        title: model.title,
        jurisdiction: model.jurisdiction,
        effective_from: model.effective_from,
        source_url: model.source_url,
        original_filename: model.original_filename,
        declared_mime_type: model.declared_mime_type,
        detected_mime_type: model.detected_mime_type,
        byte_length: u64::try_from(model.byte_length).unwrap_or_default(),
        content_sha256: model.content_sha256,
        raw_object_key: model.raw_object_key,
        normalized_object_key: model.normalized_object_key,
        parser_kind: model.parser_kind,
        parser_version: model.parser_version,
        model_provider: model.model_provider,
        model_name: model.model_name,
        prompt_version: model.prompt_version,
        page_count: model.page_count.and_then(|value| u32::try_from(value).ok()),
        coverage: serde_json::from_value(model.coverage_payload).map_err(serialization_error)?,
        candidate_count: u32::try_from(model.candidate_count).unwrap_or_default(),
        verification_status: enum_from_string(&model.verification_status)?,
        verified_by: model.verified_by,
        verified_at: model.verified_at,
        verification_notes: model.verification_notes,
        failure_code: model.failure_code,
        failure_detail: model.failure_detail,
        idempotency_key: model.idempotency_key,
        compiled_source_id: model.compiled_source_id.map(governance_domain::SourceId),
        compiled_policy_pack_id: model
            .compiled_policy_pack_id
            .map(governance_domain::PolicyPackId),
        created_at: model.created_at,
        updated_at: model.updated_at,
        completed_at: model.completed_at,
    })
}

fn candidate_active_model(
    candidate: &PolicyCandidate,
) -> Result<policy_candidates::ActiveModel, ApplicationError> {
    Ok(policy_candidates::ActiveModel {
        id: Set(candidate.id.0),
        organization_id: Set(candidate.organization_id.0),
        policy_import_id: Set(candidate.policy_import_id.0),
        position: Set(i32::try_from(candidate.position).unwrap_or(i32::MAX)),
        origin: Set(enum_string(candidate.origin)?),
        fingerprint: Set(candidate.fingerprint.clone()),
        key: Set(candidate.key.clone()),
        statement: Set(candidate.statement.clone()),
        locator_payload: Set(serde_json::to_value(&candidate.locator).map_err(serialization_error)?),
        source_excerpt: Set(candidate.source_excerpt.clone()),
        applicability: Set(candidate.applicability.clone()),
        exceptions: Set(serde_json::to_value(&candidate.exceptions).map_err(serialization_error)?),
        required_evidence: Set(
            serde_json::to_value(&candidate.required_evidence).map_err(serialization_error)?
        ),
        suggested_severity: Set(enum_string(candidate.suggested_severity)?),
        suggested_rule: Set(candidate
            .suggested_rule
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(serialization_error)?),
        mapping_status: Set(enum_string(candidate.mapping_status)?),
        model_confidence: Set(candidate.model_confidence.map(f64::from)),
        model_payload_sha256: Set(candidate.model_payload_sha256.clone()),
        status: Set(enum_string(candidate.status)?),
        review_payload: Set(candidate
            .review
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(serialization_error)?),
        created_at: Set(candidate.created_at),
        updated_at: Set(candidate.updated_at),
    })
}

#[allow(clippy::cast_possible_truncation)]
fn candidate_from_model(
    model: policy_candidates::Model,
) -> Result<PolicyCandidate, ApplicationError> {
    Ok(PolicyCandidate {
        id: PolicyCandidateId(model.id),
        organization_id: governance_domain::OrganizationId(model.organization_id),
        policy_import_id: PolicyImportId(model.policy_import_id),
        position: u32::try_from(model.position).unwrap_or_default(),
        origin: enum_from_string(&model.origin)?,
        fingerprint: model.fingerprint,
        key: model.key,
        statement: model.statement,
        locator: serde_json::from_value(model.locator_payload).map_err(serialization_error)?,
        source_excerpt: model.source_excerpt,
        applicability: model.applicability,
        exceptions: serde_json::from_value(model.exceptions).map_err(serialization_error)?,
        required_evidence: serde_json::from_value(model.required_evidence)
            .map_err(serialization_error)?,
        suggested_severity: enum_from_string(&model.suggested_severity)?,
        suggested_rule: model
            .suggested_rule
            .map(serde_json::from_value)
            .transpose()
            .map_err(serialization_error)?,
        mapping_status: enum_from_string(&model.mapping_status)?,
        model_confidence: model.model_confidence.map(|value| value as f32),
        model_payload_sha256: model.model_payload_sha256,
        status: enum_from_string(&model.status)?,
        review: model
            .review_payload
            .map(serde_json::from_value::<CandidateReview>)
            .transpose()
            .map_err(serialization_error)?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn review_active_model(
    review: &PolicyCandidateReviewRecord,
) -> Result<policy_candidate_reviews::ActiveModel, ApplicationError> {
    Ok(policy_candidate_reviews::ActiveModel {
        id: Set(review.id.0),
        organization_id: Set(review.organization_id.0),
        candidate_id: Set(review.candidate_id.0),
        decision: Set(enum_string(review.decision)?),
        reviewer_id: Set(review.reviewer_id.clone()),
        notes: Set(review.notes.clone()),
        before_payload: Set(review.before_payload.clone()),
        after_payload: Set(review.after_payload.clone()),
        reviewed_at: Set(review.reviewed_at),
    })
}

fn enum_from_string<T>(value: &str) -> Result<T, ApplicationError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(serde_json::Value::String(value.to_owned())).map_err(serialization_error)
}
