// @generated automatically by Diesel CLI.

diesel::table! {
    edges (id) {
        id -> Uuid,
        pre_id -> Uuid,
        post_id -> Uuid,
        weight -> Float4,
        edge_type -> Text,
        metadata -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    nodes (id) {
        id -> Uuid,
        label -> Text,
        node_type -> Text,
        source_file -> Nullable<Text>,
        model_blob_path -> Nullable<Text>,
        metadata -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    spike_log (id) {
        id -> Int8,
        node_id -> Uuid,
        t_ms -> Float8,
        recorded_at -> Timestamptz,
    }
}

diesel::joinable!(spike_log -> nodes (node_id));

diesel::allow_tables_to_appear_in_same_query!(
    edges,
    nodes,
    spike_log,
);
