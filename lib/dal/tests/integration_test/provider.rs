mod builtins;
mod inter_component;
mod intra_component;

use dal::test::helpers::setup_identity_func;
use dal::test_harness::{create_schema, create_schema_variant_with_root};
use dal::{
    socket::SocketArity, DalContext, ExternalProvider, InternalProvider, SchemaKind, SchematicKind,
    StandardModel,
};

use crate::dal::test;

#[test]
async fn new_external(ctx: &DalContext<'_, '_>) {
    let mut schema = create_schema(ctx, &SchemaKind::Concrete).await;
    let (schema_variant, _root_prop) = create_schema_variant_with_root(ctx, *schema.id()).await;
    schema
        .set_default_schema_variant_id(ctx, Some(*schema_variant.id()))
        .await
        .expect("cannot set default schema variant");
    let (func_id, func_binding_id, func_binding_return_value_id) = setup_identity_func(ctx).await;

    let (external_provider, output_socket) = ExternalProvider::new_with_socket(
        ctx,
        *schema.id(),
        *schema_variant.id(),
        "poop",
        None,
        func_id,
        func_binding_id,
        func_binding_return_value_id,
        SocketArity::Many,
        SchematicKind::Component,
    )
    .await
    .expect("could not create external provider");

    let found_external_provider = ExternalProvider::find_for_socket(ctx, *output_socket.id())
        .await
        .expect("could not find external provider for output socket")
        .expect("external provider for output socket not found");

    assert_eq!(
        *found_external_provider.id(), // actual
        *external_provider.id()        // expected
    );
}

#[test]
async fn new_implicit_internal(ctx: &DalContext<'_, '_>) {
    let mut schema = create_schema(ctx, &SchemaKind::Concrete).await;
    let (schema_variant, _root_prop) = create_schema_variant_with_root(ctx, *schema.id()).await;
    schema
        .set_default_schema_variant_id(ctx, Some(*schema_variant.id()))
        .await
        .expect("cannot set default schema variant");
    let (func_id, func_binding_id, func_binding_return_value_id) = setup_identity_func(ctx).await;

    let (explicit_internal_provider, input_socket) = InternalProvider::new_explicit_with_socket(
        ctx,
        *schema.id(),
        *schema_variant.id(),
        "poop",
        func_id,
        func_binding_id,
        func_binding_return_value_id,
        SocketArity::Many,
        SchematicKind::Component,
    )
    .await
    .expect("could not create (explicit internal provider");

    let found_explicit_internal_provider =
        InternalProvider::find_explicit_for_socket(ctx, *input_socket.id())
            .await
            .expect("could not find explicit internal provider for input socket")
            .expect("explicit internal provider for input socket not found");

    assert_eq!(
        *found_explicit_internal_provider.id(), // actual
        *explicit_internal_provider.id()        // expected
    );
}
