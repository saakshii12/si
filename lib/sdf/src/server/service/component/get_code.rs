use axum::{extract::Query, Json};
use dal::{CodeView, Component, ComponentId, SystemId, Visibility, WorkspaceId};
use serde::{Deserialize, Serialize};

use super::ComponentResult;
use crate::server::extract::{AccessBuilder, HandlerContext};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetCodeRequest {
    pub component_id: ComponentId,
    pub system_id: Option<SystemId>,
    pub workspace_id: WorkspaceId,
    #[serde(flatten)]
    pub visibility: Visibility,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetCodeResponse {
    pub code_views: Vec<CodeView>,
}

pub async fn get_code(
    HandlerContext(builder): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    Query(request): Query<GetCodeRequest>,
) -> ComponentResult<Json<GetCodeResponse>> {
    let ctx = builder.build(request_ctx.build(request.visibility)).await?;

    let code_views = Component::list_code_generated(&ctx, request.component_id).await?;

    Ok(Json(GetCodeResponse { code_views }))
}
