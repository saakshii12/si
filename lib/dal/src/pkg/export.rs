use std::{
    collections::{hash_map::Entry, HashMap},
    convert::TryFrom,
    path::PathBuf,
};
use strum::IntoEnumIterator;

use si_pkg::{
    ActionSpec, AttrFuncInputSpec, AttrFuncInputSpecKind, FuncArgumentSpec, FuncSpec,
    FuncSpecBackendKind, FuncSpecBackendResponseType, FuncUniqueId, LeafFunctionSpec,
    LeafInputLocation as PkgLeafInputLocation, PkgSpec, PropSpec, PropSpecBuilder, PropSpecKind,
    SchemaSpec, SchemaVariantSpec, SchemaVariantSpecBuilder, SiPkg, SocketSpec, SocketSpecKind,
    SpecError, ValidationSpec, ValidationSpecKind, WorkflowSpec,
};

use crate::{
    func::{argument::FuncArgument, backend::validation::FuncBackendValidationArgs},
    prop_tree::{PropTree, PropTreeNode},
    socket::SocketKind,
    validation::Validation,
    ActionPrototype, ActionPrototypeContext, AttributeContextBuilder, AttributePrototype,
    AttributePrototypeArgument, AttributeValue, DalContext, ExternalProvider, ExternalProviderId,
    Func, FuncId, InternalProvider, InternalProviderId, LeafKind, Prop, PropId, PropKind, Schema,
    SchemaId, SchemaVariant, SchemaVariantId, Socket, StandardModel, StandardModelError,
    ValidationPrototype, WorkflowPrototype, WorkflowPrototypeContext,
};

use super::{PkgError, PkgResult};

type FuncSpecMap = HashMap<FuncId, FuncSpec>;

// TODO(fnichol): another first-pass function with arguments. At the moment we're passing a list of
// `SchemaVariantId`s in an effort to export specific schema/variant combos but this will change in
// the future to be more encompassing. And yes, to many function args, way too many--and they're
// all `String`s
pub async fn export_pkg(
    ctx: &DalContext,
    pkg_file_path: impl Into<PathBuf>,
    name: impl Into<String>,
    version: impl Into<String>,
    description: Option<impl Into<String>>,
    created_by: impl Into<String>,
    variant_ids: Vec<SchemaVariantId>,
) -> PkgResult<()> {
    let mut pkg_spec_builder = PkgSpec::builder();
    pkg_spec_builder
        .name(name)
        .version(version)
        .created_by(created_by);
    if let Some(description) = description {
        pkg_spec_builder.description(description);
    }

    let mut func_specs = FuncSpecMap::new();

    for intrinsic_name in crate::func::INTRINSIC_FUNC_NAMES {
        // We need a unique id for intrinsic funcs to refer to them in custom bindings (for example
        // mapping one prop to another via si:identity)
        let intrinsic_func = Func::find_by_name(ctx, intrinsic_name)
            .await?
            .ok_or(PkgError::MissingIntrinsicFunc(intrinsic_name.to_string()))?;
        let intrinsic_spec = build_intrinsic_func_spec(intrinsic_name)?;
        func_specs.insert(*intrinsic_func.id(), intrinsic_spec.clone());
        pkg_spec_builder.func(intrinsic_spec);
    }

    for variant_id in variant_ids {
        let related_funcs = SchemaVariant::all_funcs(ctx, variant_id).await?;
        for func in &related_funcs {
            if !func_specs.contains_key(func.id()) {
                let arguments = FuncArgument::list_for_func(ctx, *func.id()).await?;
                let func_spec = build_func_spec(func, &arguments)?;
                func_specs.insert(*func.id(), func_spec.clone());
                pkg_spec_builder.func(func_spec);
            }
        }
        let schema_spec = build_schema_spec(ctx, variant_id, &func_specs).await?;
        pkg_spec_builder.schema(schema_spec);
    }

    let spec = pkg_spec_builder.build()?;

    let pkg = SiPkg::load_from_spec(spec)?;
    pkg.write_to_file(pkg_file_path).await?;

    Ok(())
}

fn build_intrinsic_func_spec(name: &str) -> Result<FuncSpec, PkgError> {
    Ok(FuncSpec::builder()
        .name(name)
        .handler(name)
        .code_base64("")
        .response_type(FuncSpecBackendResponseType::Json)
        .backend_kind(FuncSpecBackendKind::Json)
        .hidden(false)
        .build()?)
}

fn build_func_spec(func: &Func, args: &[FuncArgument]) -> Result<FuncSpec, PkgError> {
    let mut func_spec_builder = FuncSpec::builder();

    func_spec_builder.name(func.name());

    if let Some(display_name) = func.display_name() {
        func_spec_builder.display_name(display_name);
    }

    if let Some(description) = func.description() {
        func_spec_builder.description(description);
    }

    if let Some(link) = func.link() {
        func_spec_builder.try_link(link)?;
    }
    // Should we package an empty func?
    func_spec_builder.handler(func.handler().unwrap_or(""));
    func_spec_builder.code_base64(func.code_base64().unwrap_or(""));

    func_spec_builder.response_type(FuncSpecBackendResponseType::try_from(
        *func.backend_response_type(),
    )?);

    func_spec_builder.backend_kind(FuncSpecBackendKind::try_from(*func.backend_kind())?);

    func_spec_builder.hidden(func.hidden());

    for arg in args {
        func_spec_builder.argument(
            FuncArgumentSpec::builder()
                .name(arg.name())
                .kind(*arg.kind())
                .element_kind(arg.element_kind().cloned().map(|kind| kind.into()))
                .build()?,
        );
    }

    Ok(func_spec_builder.build()?)
}

async fn build_schema_spec(
    ctx: &DalContext,
    variant_id: SchemaVariantId,
    func_specs: &FuncSpecMap,
) -> Result<SchemaSpec, PkgError> {
    let (variant, schema) = get_schema_and_variant(ctx, variant_id).await?;

    let mut schema_spec_builder = SchemaSpec::builder();
    schema_spec_builder.name(schema.name());
    set_schema_spec_category_data(ctx, &schema, &mut schema_spec_builder).await?;

    let variant_spec = build_variant_spec(ctx, &schema, variant, func_specs).await?;
    schema_spec_builder.variant(variant_spec);

    let schema_spec = schema_spec_builder.build()?;

    Ok(schema_spec)
}

async fn build_leaf_function_specs(
    ctx: &DalContext,
    variant_id: SchemaVariantId,
    func_specs: &FuncSpecMap,
) -> Result<Vec<LeafFunctionSpec>, PkgError> {
    let mut specs = vec![];

    for leaf_kind in LeafKind::iter() {
        for leaf_func in SchemaVariant::find_leaf_item_functions(ctx, variant_id, leaf_kind).await?
        {
            let func_spec = func_specs
                .get(leaf_func.id())
                .ok_or(PkgError::MissingExportedFunc(*leaf_func.id()))?;

            let mut inputs = vec![];
            for arg in FuncArgument::list_for_func(ctx, *leaf_func.id()).await? {
                inputs.push(PkgLeafInputLocation::try_from_arg_name(arg.name())?);
            }

            specs.push(
                LeafFunctionSpec::builder()
                    .func_unique_id(func_spec.unique_id)
                    .leaf_kind(leaf_kind)
                    .inputs(inputs)
                    .build()?,
            );
        }
    }

    Ok(specs)
}

async fn build_workflow_specs(
    ctx: &DalContext,
    schema_id: SchemaId,
    schema_variant_id: SchemaVariantId,
    func_specs: &FuncSpecMap,
) -> Result<Vec<WorkflowSpec>, PkgError> {
    let mut specs = vec![];

    for workflow_prototype in WorkflowPrototype::find_for_context(
        ctx,
        WorkflowPrototypeContext {
            schema_id,
            schema_variant_id,
            ..Default::default()
        },
    )
    .await?
    {
        let func_spec = func_specs
            .get(&workflow_prototype.func_id())
            .ok_or(PkgError::MissingExportedFunc(workflow_prototype.func_id()))?;

        let mut workflow_builder = WorkflowSpec::builder();
        workflow_builder.title(workflow_prototype.title());
        workflow_builder.func_unique_id(func_spec.unique_id);

        for action_prototype in ActionPrototype::find_for_context_and_workflow_prototype(
            ctx,
            ActionPrototypeContext {
                schema_id,
                schema_variant_id,
                ..Default::default()
            },
            *workflow_prototype.id(),
        )
        .await?
        {
            let action_spec = ActionSpec::builder()
                .name(action_prototype.name())
                .kind(action_prototype.kind())
                .build()?;

            workflow_builder.action(action_spec);
        }

        specs.push(workflow_builder.build()?);
    }

    Ok(specs)
}

async fn build_input_func_and_arguments(
    ctx: &DalContext,
    proto: AttributePrototype,
    func_specs: &FuncSpecMap,
) -> Result<Option<(FuncUniqueId, Vec<AttrFuncInputSpec>)>, PkgError> {
    let proto_func = Func::get_by_id(ctx, &proto.func_id()).await?.ok_or(
        PkgError::MissingAttributePrototypeFunc(*proto.id(), proto.func_id()),
    )?;

    let apas = AttributePrototypeArgument::list_for_attribute_prototype(ctx, *proto.id()).await?;

    // If the prototype func is intrinsic and has no arguments, it's one that is created by default
    // and we don't have to track it in the package
    if apas.is_empty() && proto_func.is_intrinsic() {
        return Ok(None);
    }

    let mut inputs = vec![];

    for apa in apas {
        let func_arg = FuncArgument::get_by_id(ctx, &apa.func_argument_id())
            .await?
            .ok_or(PkgError::AttributePrototypeArgumentMissingFuncArgument(
                *apa.id(),
                apa.func_argument_id(),
            ))?;
        let arg_name = func_arg.name();

        if apa.internal_provider_id() != InternalProviderId::NONE {
            let ip = InternalProvider::get_by_id(ctx, &apa.internal_provider_id())
                .await?
                .ok_or(PkgError::AttributePrototypeArgumentMissingInternalProvider(
                    *apa.id(),
                    apa.internal_provider_id(),
                ))?;

            match *ip.prop_id() {
                PropId::NONE => {
                    inputs.push(
                        AttrFuncInputSpec::builder()
                            .name(arg_name)
                            .kind(AttrFuncInputSpecKind::InputSocket)
                            .socket_name(ip.name())
                            .build()?,
                    );
                }
                prop_id => {
                    let prop = Prop::get_by_id(ctx, &prop_id)
                        .await?
                        .ok_or(PkgError::InternalProviderMissingProp(*ip.id(), prop_id))?;

                    inputs.push(
                        AttrFuncInputSpec::builder()
                            .name(arg_name)
                            .kind(AttrFuncInputSpecKind::Prop)
                            .prop_path(prop.path())
                            .build()?,
                    );
                }
            }
        } else if apa.external_provider_id() != ExternalProviderId::NONE {
            let ep = ExternalProvider::get_by_id(ctx, &apa.external_provider_id())
                .await?
                .ok_or(PkgError::AttributePrototypeArgumentMissingExternalProvider(
                    *apa.id(),
                    apa.external_provider_id(),
                ))?;

            inputs.push(
                AttrFuncInputSpec::builder()
                    .name(arg_name)
                    .kind(AttrFuncInputSpecKind::OutputSocket)
                    .socket_name(ep.name())
                    .build()?,
            );
        }
    }

    let func_spec = func_specs
        .get(proto_func.id())
        .ok_or(PkgError::MissingExportedFunc(*proto_func.id()))?;

    let func_unique_id = func_spec.unique_id;

    Ok(Some((func_unique_id, inputs)))
}

async fn build_socket_specs(
    ctx: &DalContext,
    schema_variant_id: SchemaVariantId,
    func_specs: &FuncSpecMap,
) -> Result<Vec<SocketSpec>, PkgError> {
    let mut specs = vec![];

    for input_socket_ip in
        InternalProvider::list_explicit_for_schema_variant(ctx, schema_variant_id).await?
    {
        let socket = Socket::find_for_internal_provider(ctx, *input_socket_ip.id())
            .await?
            .pop()
            .ok_or(PkgError::ExplicitInternalProviderMissingSocket(
                *input_socket_ip.id(),
            ))?;

        if let SocketKind::Frame = socket.kind() {
            continue;
        }

        let mut socket_spec_builder = SocketSpec::builder();
        socket_spec_builder
            .name(input_socket_ip.name())
            .kind(SocketSpecKind::Input)
            .arity(socket.arity());

        if let Some(attr_proto_id) = input_socket_ip.attribute_prototype_id() {
            let proto = AttributePrototype::get_by_id(ctx, attr_proto_id)
                .await?
                .ok_or(PkgError::MissingAttributePrototypeForInputSocket(
                    *attr_proto_id,
                    *input_socket_ip.id(),
                ))?;

            if let Some((func_unique_id, mut inputs)) =
                build_input_func_and_arguments(ctx, proto, func_specs).await?
            {
                socket_spec_builder.func_unique_id(func_unique_id);
                inputs.drain(..).for_each(|input| {
                    socket_spec_builder.input(input);
                });
            }
        }

        specs.push(socket_spec_builder.build()?);
    }

    for output_socket_ep in
        ExternalProvider::list_for_schema_variant(ctx, schema_variant_id).await?
    {
        let socket = Socket::find_for_external_provider(ctx, *output_socket_ep.id())
            .await?
            .pop()
            .ok_or(PkgError::ExternalProviderMissingSocket(
                *output_socket_ep.id(),
            ))?;

        if let SocketKind::Frame = socket.kind() {
            continue;
        }

        let mut socket_spec_builder = SocketSpec::builder();
        socket_spec_builder
            .name(output_socket_ep.name())
            .kind(SocketSpecKind::Output)
            .arity(socket.arity());

        if let Some(attr_proto_id) = output_socket_ep.attribute_prototype_id() {
            let proto = AttributePrototype::get_by_id(ctx, attr_proto_id)
                .await?
                .ok_or(PkgError::MissingAttributePrototypeForOutputSocket(
                    *attr_proto_id,
                    *output_socket_ep.id(),
                ))?;

            if let Some((func_unique_id, mut inputs)) =
                build_input_func_and_arguments(ctx, proto, func_specs).await?
            {
                socket_spec_builder.func_unique_id(func_unique_id);
                inputs.drain(..).for_each(|input| {
                    socket_spec_builder.input(input);
                });
            }
        }

        specs.push(socket_spec_builder.build()?);
    }

    Ok(specs)
}

async fn build_variant_spec(
    ctx: &DalContext,
    schema: &Schema,
    variant: SchemaVariant,
    func_specs: &FuncSpecMap,
) -> Result<SchemaVariantSpec, PkgError> {
    let mut variant_spec_builder = SchemaVariantSpec::builder();
    variant_spec_builder.name(variant.name());
    if let Some(color_str) = variant.color(ctx).await? {
        variant_spec_builder.color(color_str);
    };
    if let Some(link) = variant.link() {
        variant_spec_builder.try_link(link)?;
    }

    set_variant_spec_prop_data(ctx, &variant, &mut variant_spec_builder, func_specs).await?;

    build_leaf_function_specs(ctx, *variant.id(), func_specs)
        .await?
        .drain(..)
        .for_each(|leaf_func_spec| {
            variant_spec_builder.leaf_function(leaf_func_spec);
        });

    build_workflow_specs(ctx, *schema.id(), *variant.id(), func_specs)
        .await?
        .drain(..)
        .for_each(|workflow_spec| {
            variant_spec_builder.workflow(workflow_spec);
        });

    build_socket_specs(ctx, *variant.id(), func_specs)
        .await?
        .drain(..)
        .for_each(|socket_spec| {
            variant_spec_builder.socket(socket_spec);
        });

    let variant_spec = variant_spec_builder.build()?;

    Ok(variant_spec)
}

async fn get_schema_and_variant(
    ctx: &DalContext,
    variant_id: SchemaVariantId,
) -> Result<(SchemaVariant, Schema), PkgError> {
    let variant = SchemaVariant::get_by_id(ctx, &variant_id)
        .await?
        .ok_or_else(|| {
            StandardModelError::ModelMissing("schema_variants".to_string(), variant_id.to_string())
        })?;

    let schema = variant.schema(ctx).await?.ok_or_else(|| {
        PkgError::StandardModelMissingBelongsTo(
            "schema_variant_belongs_to_schema",
            "schema_variant",
            variant_id.to_string(),
        )
    })?;

    Ok((variant, schema))
}

async fn set_schema_spec_category_data(
    ctx: &DalContext,
    schema: &Schema,
    schema_spec_builder: &mut si_pkg::SchemaSpecBuilder,
) -> Result<(), PkgError> {
    let mut schema_ui_menus = schema.ui_menus(ctx).await?;
    let schema_ui_menu = schema_ui_menus.pop().ok_or_else(|| {
        PkgError::StandardModelMissingBelongsTo(
            "schema_ui_menu_belongs_to_schema",
            "schema",
            (*schema.id()).to_string(),
        )
    })?;
    if !schema_ui_menus.is_empty() {
        return Err(PkgError::StandardModelMultipleBelongsTo(
            "schema_ui_menu_belongs_to_schema",
            "schema",
            (*schema.id()).to_string(),
        ));
    }

    schema_spec_builder.category(schema_ui_menu.category());
    schema_spec_builder.category_name(schema_ui_menu.name());

    Ok(())
}

async fn set_variant_spec_prop_data(
    ctx: &DalContext,
    variant: &SchemaVariant,
    variant_spec: &mut SchemaVariantSpecBuilder,
    func_specs: &HashMap<FuncId, FuncSpec>,
) -> Result<(), PkgError> {
    let mut prop_tree = PropTree::new(ctx, false, Some(*variant.id())).await?;
    let root_tree_node = prop_tree
        .root_props
        .pop()
        .ok_or_else(|| PkgError::prop_tree_invalid("root prop not found"))?;
    if !prop_tree.root_props.is_empty() {
        return Err(PkgError::prop_tree_invalid(
            "prop tree contained multiple root props",
        ));
    }
    let domain_tree_node = root_tree_node
        .children
        .into_iter()
        .find(|tree_node| tree_node.name == "domain" && tree_node.path == "/root/")
        .ok_or_else(|| PkgError::prop_tree_invalid("domain prop not found"))?;

    #[derive(Debug)]
    struct TraversalStackEntry {
        builder: PropSpecBuilder,
        prop_id: PropId,
        parent_prop_id: Option<PropId>,
    }

    let mut stack: Vec<(PropTreeNode, Option<PropId>)> = Vec::new();
    for domain_child_tree_node in domain_tree_node.children {
        stack.push((domain_child_tree_node, None));
    }

    let mut traversal_stack: Vec<TraversalStackEntry> = Vec::new();

    while let Some((tree_node, parent_prop_id)) = stack.pop() {
        let prop_id = tree_node.prop_id;
        let mut builder = PropSpec::builder();
        builder
            .kind(match tree_node.kind {
                PropKind::Array => PropSpecKind::Array,
                PropKind::Boolean => PropSpecKind::Boolean,
                PropKind::Integer => PropSpecKind::Number,
                PropKind::Object => PropSpecKind::Object,
                PropKind::String => PropSpecKind::String,
                PropKind::Map => PropSpecKind::Map,
            })
            .name(tree_node.name);
        traversal_stack.push(TraversalStackEntry {
            builder,
            prop_id,
            parent_prop_id,
        });

        for child_tree_node in tree_node.children {
            stack.push((child_tree_node, Some(prop_id)));
        }
    }

    let mut prop_children_map: HashMap<PropId, Vec<PropSpec>> = HashMap::new();

    while let Some(mut entry) = traversal_stack.pop() {
        if let Some(mut prop_children) = prop_children_map.remove(&entry.prop_id) {
            match entry.builder.get_kind() {
                Some(kind) => match kind {
                    PropSpecKind::Object => {
                        entry.builder.entries(prop_children);
                    }
                    PropSpecKind::Map | PropSpecKind::Array => {
                        let type_prop = prop_children.pop().ok_or_else(|| {
                            PkgError::prop_spec_children_invalid(format!(
                                "found no child for map/array for prop id {}",
                                entry.prop_id,
                            ))
                        })?;
                        if !prop_children.is_empty() {
                            return Err(PkgError::prop_spec_children_invalid(format!(
                                "found multiple children for map/array for prop id {}",
                                entry.prop_id,
                            )));
                        }
                        entry.builder.type_prop(type_prop);
                    }
                    PropSpecKind::String | PropSpecKind::Number | PropSpecKind::Boolean => {
                        return Err(PkgError::prop_spec_children_invalid(format!(
                            "primitve prop type should have no children for prop id {}",
                            entry.prop_id,
                        )));
                    }
                },
                None => {
                    return Err(SpecError::UninitializedField("kind").into());
                }
            };
        }

        let context = AttributeContextBuilder::new()
            .set_prop_id(entry.prop_id)
            .to_context()?;

        if let Some(prototype) = AttributePrototype::find_for_context_and_key(ctx, context, &None)
            .await?
            .pop()
        {
            if let Some((func_unique_id, mut inputs)) =
                build_input_func_and_arguments(ctx, prototype, func_specs).await?
            {
                entry.builder.func_unique_id(func_unique_id);
                inputs.drain(..).for_each(|input| {
                    entry.builder.input(input);
                });
            }
        }

        // TODO: handle default values for complex types.
        if matches!(
            entry.builder.get_kind(),
            Some(PropSpecKind::String) | Some(PropSpecKind::Number) | Some(PropSpecKind::Boolean)
        ) {
            if let Some(av) = AttributeValue::find_for_context(ctx, context.into()).await? {
                if let Some(default_value) = av.get_value(ctx).await? {
                    entry.builder.default_value(default_value);
                }
            }
        }

        for validation in get_validations_for_prop(ctx, entry.prop_id, func_specs).await? {
            entry.builder.validation(validation);
        }

        let prop_spec = entry.builder.build()?;

        match entry.parent_prop_id {
            None => {
                variant_spec.prop(prop_spec);
            }
            Some(parent_prop_id) => {
                match prop_children_map.entry(parent_prop_id) {
                    Entry::Occupied(mut occupied) => {
                        occupied.get_mut().push(prop_spec);
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(vec![prop_spec]);
                    }
                };
            }
        };
    }

    Ok(())
}

async fn get_validations_for_prop(
    ctx: &DalContext,
    prop_id: PropId,
    func_specs: &HashMap<FuncId, FuncSpec>,
) -> PkgResult<Vec<ValidationSpec>> {
    let mut validation_specs = vec![];

    for prototype in ValidationPrototype::list_for_prop(ctx, prop_id).await? {
        let mut spec_builder = ValidationSpec::builder();
        let args: Option<FuncBackendValidationArgs> =
            serde_json::from_value(prototype.args().clone())?;

        match args {
            Some(validation) => match validation.validation {
                Validation::IntegerIsBetweenTwoIntegers {
                    lower_bound,
                    upper_bound,
                    ..
                } => {
                    spec_builder.kind(ValidationSpecKind::IntegerIsBetweenTwoIntegers);
                    spec_builder.upper_bound(upper_bound);
                    spec_builder.lower_bound(lower_bound);
                }
                Validation::StringHasPrefix { expected, .. } => {
                    spec_builder.kind(ValidationSpecKind::StringHasPrefix);
                    spec_builder.expected_string(expected);
                }
                Validation::StringEquals { expected, .. } => {
                    spec_builder.kind(ValidationSpecKind::StringEquals);
                    spec_builder.expected_string(expected);
                }
                Validation::StringInStringArray {
                    expected,
                    display_expected,
                    ..
                } => {
                    spec_builder.kind(ValidationSpecKind::StringInStringArray);
                    spec_builder.expected_string_array(expected);
                    spec_builder.display_expected(display_expected);
                }
                Validation::StringIsNotEmpty { .. } => {
                    spec_builder.kind(ValidationSpecKind::StringIsNotEmpty);
                }
                Validation::StringIsValidIpAddr { .. } => {
                    spec_builder.kind(ValidationSpecKind::StringIsValidIpAddr);
                }
                Validation::StringIsHexColor { .. } => {
                    spec_builder.kind(ValidationSpecKind::StringIsHexColor);
                }
            },
            None => {
                let func_spec = func_specs
                    .get(&prototype.func_id())
                    .ok_or(PkgError::MissingExportedFunc(prototype.func_id()))?;

                spec_builder.kind(ValidationSpecKind::CustomValidation);
                spec_builder.func_unique_id(func_spec.unique_id);
            }
        }

        validation_specs.push(spec_builder.build()?);
    }

    Ok(validation_specs)
}
