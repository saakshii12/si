use axum::extract::Query;
use axum::Json;
use dal::property_editor::PropertyEditorValues;
use dal::{AttributeReadContext, Component, ComponentId, StandardModel, SystemId, Visibility};
use serde::{Deserialize, Serialize};

use super::{ComponentError, ComponentResult};
use crate::server::extract::{AccessBuilder, HandlerContext};

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetPropertyEditorValuesRequest {
    pub component_id: ComponentId,
    pub system_id: SystemId,
    #[serde(flatten)]
    pub visibility: Visibility,
}

pub type GetPropertyEditorValuesResponse = PropertyEditorValues;

pub async fn get_property_editor_values(
    HandlerContext(builder, mut txns): HandlerContext,
    AccessBuilder(request_ctx): AccessBuilder,
    Query(request): Query<GetPropertyEditorValuesRequest>,
) -> ComponentResult<Json<GetPropertyEditorValuesResponse>> {
    let txns = txns.start().await?;
    let ctx = builder.build(request_ctx.build(request.visibility), &txns);

    let component = Component::get_by_id(&ctx, &request.component_id)
        .await?
        .ok_or(ComponentError::ComponentNotFound)?;
    let schema_id = *component
        .schema(&ctx)
        .await?
        .ok_or(ComponentError::SchemaNotFound)?
        .id();
    let schema_variant_id = *component
        .schema_variant(&ctx)
        .await?
        .ok_or(ComponentError::SchemaNotFound)?
        .id();
    let context = AttributeReadContext {
        schema_id: Some(schema_id),
        schema_variant_id: Some(schema_variant_id),
        component_id: Some(request.component_id),
        prop_id: None,
        system_id: Some(request.system_id),
        ..AttributeReadContext::default()
    };
    let prop_edit_values = PropertyEditorValues::for_context(&ctx, context).await?;

    txns.commit().await?;

    Ok(Json(prop_edit_values))
}
