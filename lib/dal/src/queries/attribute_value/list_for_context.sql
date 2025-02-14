SELECT DISTINCT ON (
    COALESCE(belongs_to_id, ''),
    attribute_context_prop_id,
    attribute_context_internal_provider_id,
    attribute_context_external_provider_id,
    COALESCE(key, '')
    ) row_to_json(av.*) AS object
FROM attribute_values_v1($1, $2) AS av
         LEFT JOIN attribute_value_belongs_to_attribute_value_v1($1, $2) AS avbtav
                   ON avbtav.object_id = av.id
WHERE in_attribute_context_v1($3, av)
ORDER BY COALESCE(belongs_to_id, ''),
         attribute_context_prop_id DESC,
         attribute_context_internal_provider_id DESC,
         attribute_context_external_provider_id DESC,
         COALESCE(key, ''),
         attribute_context_component_id DESC
