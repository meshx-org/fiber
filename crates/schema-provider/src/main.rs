use anyhow::Context;
use itertools::Itertools;
use serde::Serialize;
use sqlx::PgPool;
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;

pub(crate) mod bindings {
    wit_bindgen_wrpc::generate!({
        with: {
            "wasi:logging/logging@0.1.0-draft": generate,
            "meshx:data/types": generate,
            "meshx:data/schema": generate
        },
        additional_derives: [serde::Deserialize, serde::Serialize],
    });
}


#[derive(Debug, Clone, Serialize)]
struct Column {
    name: String,
    r#type: String,
    is_nullable: bool,
    column_default: Option<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
struct Table {
    name: String,
    columns: Vec<Column>,
    primary_key: Vec<String>,
    unique_constraints: Vec<Vec<String>>, // list of unique column sets
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct FKRow {
    constraint_name: String,
    table_schema: String,
    table_name: String,
    column_name: String,
    foreign_table_schema: String,
    foreign_table_name: String,
    foreign_column_name: String,
    ordinal_position: i32,
}

#[derive(Debug, Serialize)]
struct Relationship {
    kind: String, // "one-to-one", "one-to-many", "many-to-many"
    from_table: String,
    from_columns: Vec<String>,
    to_table: String,
    to_columns: Vec<String>,
    via_table: Option<String>, // if many-to-many, the join-table
}

#[derive(Debug, Clone, Serialize)]
pub struct EnumVariant {
    pub value: String,
    pub name: Option<String>,
    pub color: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct EnumType {
    name: String,
    variants: Vec<EnumVariant>,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct SchemaDump {
    enums: Vec<EnumType>,
    tables: Vec<Table>,
    relationships: Vec<Relationship>,
}

#[derive(Debug)]
pub enum MigrationOp {
    CreateTable {
        table: Table,
    },
    DropTable {
        name: String,
    },

    AddColumn {
        table: String,
        column: Column,
    },
    DropColumn {
        table: String,
        column: String,
    },
    AlterColumnType {
        table: String,
        column: String,
        old_type: String,
        new_type: String,
    },
    AlterColumnNullability {
        table: String,
        column: String,
        nullable: bool,
    },

    AddPrimaryKey {
        table: String,
        columns: Vec<String>,
    },
    DropPrimaryKey {
        table: String,
    },

    AddUnique {
        table: String,
        columns: Vec<String>,
    },
    DropUnique {
        table: String,
        columns: Vec<String>,
    },

    AddForeignKey {
        table: String,
        columns: Vec<String>,
        ref_table: String,
        ref_columns: Vec<String>,
    },
    DropForeignKey {
        table: String,
        constraint_name: String,
    },

    CreateEnum {
        name: String,
        variants: Vec<String>,
    },
    DropEnum {
        name: String,
    },
    AddEnumVariant {
        name: String,
        variant: String,
        position_after: Option<String>,
    },
}

pub fn load_schema_from_code() -> SchemaDump {
    use std::collections::HashMap;

    let mut user_table_metadata = HashMap::new();
    user_table_metadata.insert("display_name".to_string(), "Users".to_string());

    let mut email_metadata = HashMap::new();
    email_metadata.insert("format".to_string(), "email".to_string());
    email_metadata.insert("display_name".to_string(), "E-Mail".to_string());

    // Define columns:
    let id_col = Column {
        name: "id".into(),
        r#type: "uuid".into(),
        is_nullable: false,
        column_default: None,
        metadata: HashMap::new(),
    };

    let email_col = Column {
        name: "email".into(),
        r#type: "text".into(),
        is_nullable: false,
        column_default: None,
        metadata: email_metadata,
    };

    let created_at_col = Column {
        name: "created_at".into(),
        r#type: "timestampz".into(),
        is_nullable: false,
        metadata: HashMap::new(),
        column_default: None,
    };

    let users = Table {
        name: "users".into(),
        columns: vec![id_col, email_col, created_at_col],
        primary_key: vec!["id".into()],
        unique_constraints: vec![vec!["email".into()]],
        metadata: user_table_metadata,
    };

    SchemaDump {
        enums: vec![],
        tables: vec![users],
        relationships: vec![], // none for now
    }
}

pub fn diff_schemas(current: &SchemaDump, desired: &SchemaDump) -> Vec<MigrationOp> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut ops = Vec::new();

    // Enums
    {
        let cur_enums: BTreeMap<_, _> = current.enums.iter().map(|e| (e.name.clone(), e)).collect();
        let new_enums: BTreeMap<_, _> = desired.enums.iter().map(|e| (e.name.clone(), e)).collect();

        // Detect dropped enums
        for (name, _) in &cur_enums {
            if !new_enums.contains_key(name) {
                ops.push(MigrationOp::DropEnum { name: name.clone() });
            }
        }

        // Detect created enums
        for (name, en) in &new_enums {
            if !cur_enums.contains_key(name) {
                ops.push(MigrationOp::CreateEnum {
                    name: name.clone(),
                    variants: en.variants.iter().map(|v| v.value.clone()).collect(),
                });
                continue;
            }
        }

        // Detect enum changes (Postgres only allows adding, not removing or reordering)
        for (name, new) in &new_enums {
            if let Some(cur) = cur_enums.get(name) {
                let cur_set: BTreeSet<_> = cur.variants.iter().map(|v| &v.value).collect();
                for (i, v) in new.variants.iter().enumerate() {
                    if !cur_set.contains(&v.value) {
                        let before = if i > 0 {
                            Some(new.variants[i - 1].value.clone())
                        } else {
                            None
                        };
                        ops.push(MigrationOp::AddEnumVariant {
                            name: name.clone(),
                            variant: v.value.clone(),
                            position_after: before,
                        });
                    }
                }
            }
        }
    }

    // Tables
    {
        let current_tables: BTreeMap<_, _> =
            current.tables.iter().map(|t| (t.name.clone(), t)).collect();
        let desired_tables: BTreeMap<_, _> =
            desired.tables.iter().map(|t| (t.name.clone(), t)).collect();

        // Detect dropped tables
        for (name, _) in &current_tables {
            if !desired_tables.contains_key(name) {
                ops.push(MigrationOp::DropTable { name: name.clone() });
            }
        }

        // Detect new tables
        for (name, t) in &desired_tables {
            if !current_tables.contains_key(name) {
                ops.push(MigrationOp::CreateTable {
                    table: (*t).clone(),
                });
                continue;
            }
        }

        // Compare shared tables
        for (name, desired_table) in &desired_tables {
            let Some(current_table) = current_tables.get(name) else {
                continue;
            };

            // Columns
            let cur_cols: BTreeMap<_, _> = current_table
                .columns
                .iter()
                .map(|c| (c.name.clone(), c))
                .collect();
            let new_cols: BTreeMap<_, _> = desired_table
                .columns
                .iter()
                .map(|c| (c.name.clone(), c))
                .collect();

            // Added columns
            for (col_name, col) in &new_cols {
                if !cur_cols.contains_key(col_name) {
                    ops.push(MigrationOp::AddColumn {
                        table: name.clone(),
                        column: (*col).clone(),
                    });
                }
            }

            // Removed columns
            for col_name in cur_cols.keys() {
                if !new_cols.contains_key(col_name) {
                    ops.push(MigrationOp::DropColumn {
                        table: name.clone(),
                        column: col_name.clone(),
                    });
                }
            }

            // Modified columns
            for (col_name, new_col) in &new_cols {
                if let Some(old_col) = cur_cols.get(col_name) {
                    if old_col.r#type != new_col.r#type {
                        ops.push(MigrationOp::AlterColumnType {
                            table: name.clone(),
                            column: col_name.clone(),
                            old_type: old_col.r#type.clone(),
                            new_type: new_col.r#type.clone(),
                        });
                    }

                    if old_col.is_nullable != new_col.is_nullable {
                        ops.push(MigrationOp::AlterColumnNullability {
                            table: name.clone(),
                            column: col_name.clone(),
                            nullable: new_col.is_nullable,
                        });
                    }
                }
            }

            // Primary key change
            if current_table.primary_key != desired_table.primary_key {
                if !current_table.primary_key.is_empty() {
                    ops.push(MigrationOp::DropPrimaryKey {
                        table: name.clone(),
                    });
                }
                if !desired_table.primary_key.is_empty() {
                    ops.push(MigrationOp::AddPrimaryKey {
                        table: name.clone(),
                        columns: desired_table.primary_key.clone(),
                    });
                }
            }

            // Unique constraints diff
            let cur_uniq: BTreeSet<_> = current_table.unique_constraints.iter().cloned().collect();
            let new_uniq: BTreeSet<_> = desired_table.unique_constraints.iter().cloned().collect();

            for u in new_uniq.difference(&cur_uniq) {
                ops.push(MigrationOp::AddUnique {
                    table: name.clone(),
                    columns: u.clone(),
                });
            }
            for u in cur_uniq.difference(&new_uniq) {
                ops.push(MigrationOp::DropUnique {
                    table: name.clone(),
                    columns: u.clone(),
                });
            }
        }
    }

    ops
}

pub fn ops_to_sql(ops: &[MigrationOp]) -> Vec<String> {
    let mut sql = Vec::new();
    for op in ops {
        match op {
            MigrationOp::CreateTable { table } => {
                let cols = table
                    .columns
                    .iter()
                    .map(|c| {
                        format!(
                            "{} {}{}",
                            c.name,
                            c.r#type,
                            if c.is_nullable { "" } else { " NOT NULL" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",\n    ");
                sql.push(format!("CREATE TABLE {} (\n    {}\n);", table.name, cols));
            }
            MigrationOp::DropTable { name } => sql.push(format!("DROP TABLE {};", name)),

            MigrationOp::AddColumn { table, column } => sql.push(format!(
                "ALTER TABLE {} ADD COLUMN {} {}{};",
                table,
                column.name,
                column.r#type,
                if column.is_nullable { "" } else { " NOT NULL" }
            )),

            MigrationOp::DropColumn { table, column } => {
                sql.push(format!("ALTER TABLE {} DROP COLUMN {};", table, column))
            }

            MigrationOp::AlterColumnType {
                table,
                column,
                new_type,
                ..
            } => sql.push(format!(
                "ALTER TABLE {} ALTER COLUMN {} TYPE {};",
                table, column, new_type
            )),

            MigrationOp::AlterColumnNullability {
                table,
                column,
                nullable,
            } => sql.push(format!(
                "ALTER TABLE {} ALTER COLUMN {} {} NOT NULL;",
                table,
                column,
                if *nullable { "DROP" } else { "SET" }
            )),

            MigrationOp::AddPrimaryKey { table, columns } => sql.push(format!(
                "ALTER TABLE {} ADD PRIMARY KEY({});",
                table,
                columns.join(", ")
            )),

            MigrationOp::DropPrimaryKey { table } => sql.push(format!(
                "ALTER TABLE {} DROP CONSTRAINT {}_pkey;",
                table, table
            )),

            MigrationOp::AddUnique { table, columns } => sql.push(format!(
                "ALTER TABLE {} ADD UNIQUE({});",
                table,
                columns.join(", ")
            )),

            MigrationOp::DropUnique { table, columns } => sql.push(format!(
                "ALTER TABLE {} DROP CONSTRAINT {}_{};",
                table,
                table,
                columns.join("_")
            )),

            MigrationOp::CreateEnum { name, variants } => {
                sql.push(format!(
                    "CREATE TYPE {} AS ENUM ({});",
                    name,
                    variants.iter().map(|v| format!("'{}'", v)).join(", ")
                ));
            }

            MigrationOp::DropEnum { name } => {
                sql.push(format!("DROP TYPE {};", name));
            }

            MigrationOp::AddEnumVariant {
                name,
                variant,
                position_after,
            } => {
                if let Some(after) = position_after {
                    sql.push(format!(
                        "ALTER TYPE {} ADD VALUE '{}' AFTER '{}';",
                        name, variant, after
                    ));
                } else {
                    sql.push(format!("ALTER TYPE {} ADD VALUE '{}';", name, variant));
                }
            }

            // Foreign keys omitted here for brevity; same idea
            _ => {}
        }
    }
    sql
}

async fn run_migration(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        r#"
    CREATE TABLE IF NOT EXISTS __meshx_enum_metadata (
        enum_type TEXT NOT NULL,
        enum_value TEXT NOT NULL,
        metadata JSONB NOT NULL,
        PRIMARY KEY (enum_type, enum_value)
    );
    "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_url = env::var("DATABASE_URL")
        .context("Please set DATABASE_URL env var, e.g. postgres://user:pass@localhost/db")?;
    let pool = PgPool::connect(&db_url).await?;

    run_migration(&pool).await?;

    // 1) Get tables in public schema
    let enums = fetch_enums(&pool).await?;
    let tables = fetch_tables(&pool).await?;
    let column_comments = load_column_comments(&pool).await?;

    let tables_map: BTreeMap<String, Table> =
        tables.into_iter().map(|t| (t.name.clone(), t)).collect();

    // 2) Get foreign keys
    let fks = fetch_foreign_keys(&pool).await?;

    // 3) Collect unique constraints and primary keys (already populated in Table but let's ensure)
    // we've already loaded PKs and UNIQUEs inside fetch_tables.

    // 4) Build map of FK groups per constraint (composite FK handling)
    let mut fk_groups: BTreeMap<(String, String), Vec<FKRow>> = BTreeMap::new();
    // key: (constraint_name, table_name)
    for fk in fks.into_iter() {
        fk_groups
            .entry((fk.constraint_name.clone(), fk.table_name.clone()))
            .or_default()
            .push(fk);
    }

    // 5) Detect join tables (many-to-many candidates)
    // Heuristic: join table has exactly 2 FK constraints (or at least 2 distinct foreign tables),
    // the PK on join table covers those FK columns (or the join table has only fk columns).
    let mut join_table_names = BTreeSet::new();

    // Build quick helper maps
    let mut table_to_fk_constraints: BTreeMap<String, Vec<Vec<FKRow>>> = BTreeMap::new();
    for ((_constraint_name, table_name), rows) in &fk_groups {
        table_to_fk_constraints
            .entry(table_name.clone())
            .or_default()
            .push(rows.clone());
    }

    for (table_name, fk_constraints) in &table_to_fk_constraints {
        // count total distinct FK columns referenced in this table
        let fk_cols: Vec<String> = fk_constraints
            .iter()
            .flat_map(|g| g.iter().map(|r| r.column_name.clone()))
            .collect();

        // find referenced tables
        let referenced_tables: BTreeSet<_> = fk_constraints
            .iter()
            .flat_map(|g| g.iter().map(|r| r.foreign_table_name.clone()))
            .collect();

        let tbl = match tables_map.get(table_name) {
            Some(t) => t,
            None => continue,
        };

        // heuristics: 2 referenced tables, join table primary key equals the fk columns (or pk empty but columns == 2)
        if referenced_tables.len() == 2 {
            let fk_cols_set: BTreeSet<_> = fk_cols.iter().cloned().collect();
            let pk_set: BTreeSet<_> = tbl.primary_key.iter().cloned().collect();
            let only_fk_columns = tbl
                .columns
                .iter()
                .map(|c| c.name.clone())
                .all(|c| fk_cols_set.contains(&c));

            let pk_matches_fks = !pk_set.is_empty() && fk_cols_set == pk_set;
            if pk_matches_fks || (only_fk_columns && fk_cols_set.len() == 2) {
                join_table_names.insert(table_name.clone());
            }
        }
    }

    // 6) Build relationships
    let mut relationships: Vec<Relationship> = Vec::new();
    // For simpler grouping of composite FKs to relationship objects:
    for ((_constraint_name, table_name), rows) in fk_groups.into_iter() {
        // if this is a join table that gives many-to-many, skip now (we'll handle pairs)
        if join_table_names.contains(&table_name) {
            continue;
        }

        // group rows by target table for the same constraint (composite FK supports multiple columns)
        let to_table = rows[0].foreign_table_name.clone();
        let from_columns: Vec<String> = {
            let mut v = rows
                .iter()
                .map(|r| r.column_name.clone())
                .collect::<Vec<_>>();
            v.sort();
            v
        };
        let to_columns: Vec<String> = {
            let mut v = rows
                .iter()
                .map(|r| r.foreign_column_name.clone())
                .collect::<Vec<_>>();
            v.sort();
            v
        };

        // check if child (table_name) has a unique constraint on from_columns => one-to-one
        let child_table = match tables_map.get(&table_name) {
            Some(t) => t,
            None => continue,
        };
        let from_set: BTreeSet<_> = from_columns.iter().cloned().collect();
        let mut is_unique = false;
        for uc in &child_table.unique_constraints {
            let uc_set: BTreeSet<_> = uc.iter().cloned().collect();
            if uc_set == from_set {
                is_unique = true;
                break;
            }
        }
        // also if child's primary key equals the from columns => one-to-one
        let pk_set: BTreeSet<_> = child_table.primary_key.iter().cloned().collect();
        if pk_set == from_set && !pk_set.is_empty() {
            is_unique = true;
        }

        let kind = if is_unique {
            "one-to-one".to_string()
        } else {
            "one-to-many".to_string()
        };

        relationships.push(Relationship {
            kind,
            from_table: table_name.clone(),
            from_columns,
            to_table,
            to_columns,
            via_table: None,
        });
    }

    // 7) Add many-to-many relationships from join tables
    for join_table in &join_table_names {
        // collect the two referenced tables and their fk columns
        let fk_constraints = table_to_fk_constraints
            .get(join_table)
            .cloned()
            .unwrap_or_default();
        let mut referenced: Vec<(String, Vec<String>)> = Vec::new();
        for group in fk_constraints.into_iter() {
            // group is rows for a single FK constraint (maybe composite)
            let to_table_name = group[0].foreign_table_name.clone();
            let cols = group
                .iter()
                .map(|r| r.column_name.clone())
                .collect::<Vec<_>>();
            referenced.push((to_table_name, cols));
        }
        if referenced.len() == 2 {
            let a = &referenced[0];
            let b = &referenced[1];
            relationships.push(Relationship {
                kind: "many-to-many".to_string(),
                from_table: a.0.clone(),
                from_columns: a.1.clone(),
                to_table: b.0.clone(),
                to_columns: b.1.clone(),
                via_table: Some(join_table.clone()),
            });
        }
    }

    // 8) Prepare final SchemaDump
    let current = SchemaDump {
        enums,
        tables: tables_map.into_iter().map(|(_, t)| t).collect(),
        relationships,
    };

    let desired = load_schema_from_code();

    let diff = diff_schemas(&current, &desired);
    let sql = ops_to_sql(&diff);

    for stmt in sql {
        println!("{}", stmt);
    }

    // 9) Output JSON to stdout
    let json = serde_json::to_string_pretty(&current)?;
    println!("--- JSON schema dump ---\n{}\n", json);

    // 10) Generate PlantUML
    let plantuml = generate_plantuml(&current);
    println!("--- PlantUML ---\n{}\n", plantuml);

    // Write outputs to files for convenience
    std::fs::write("schema_dump.json", &json)?;
    std::fs::write("schema_diagram.puml", &plantuml)?;
    println!("Wrote schema_dump.json and schema_diagram.puml");

    Ok(())
}

async fn fetch_enums(pool: &PgPool) -> anyhow::Result<Vec<EnumType>> {
    // 1) Fetch raw enum values from pg_enum
    let enum_rows = sqlx::query(
        r#"
        SELECT t.typname AS enum_name, e.enumlabel AS variant
        FROM pg_type t
        JOIN pg_enum e ON t.oid = e.enumtypid
        JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE n.nspname = 'public'
        ORDER BY t.typname, e.enumsortorder
        "#,
    )
    .fetch_all(pool)
    .await?;

    // Map enum_name -> Vec<String>
    let mut enums_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in enum_rows {
        let enum_name: String = r.get("enum_name");
        let variant: String = r.get("variant");
        enums_map.entry(enum_name).or_default().push(variant);
    }

    // 2) Fetch metadata from enum_metadata table
    let meta_rows = sqlx::query(
        r#"
        SELECT enum_type, enum_value, metadata
        FROM enum_metadata
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut metadata_map = BTreeMap::new();
    for r in meta_rows {
        let enum_type: String = r.try_get("enum_type")?;
        let enum_value: String = r.try_get("enum_value")?;
        let meta: serde_json::Value = r.try_get("metadata")?;
        metadata_map.insert((enum_type, enum_value), meta);
    }

    // 3) Merge enum values and metadata into full struct
    let mut result = Vec::new();
    for (enum_name, variants) in enums_map {
        let mut enum_variants = Vec::new();
        for v in variants {
            let meta = metadata_map.get(&(enum_name.clone(), v.clone()));
            let variant = if let Some(meta_json) = meta {
                EnumVariant {
                    value: v.clone(),
                    name: meta_json
                        .get("name")
                        .and_then(|v| v.as_str().map(|s| s.to_string())),
                    color: meta_json
                        .get("color")
                        .and_then(|v| v.as_str().map(|s| s.to_string())),
                    is_default: meta_json.get("default").and_then(|v| v.as_bool()),
                }
            } else {
                EnumVariant {
                    value: v.clone(),
                    name: None,
                    color: None,
                    is_default: None,
                }
            };
            enum_variants.push(variant);
        }

        result.push(EnumType {
            name: enum_name,
            variants: enum_variants,
            metadata: HashMap::new(),
        });
    }

    Ok(result)
}

async fn fetch_tables(pool: &PgPool) -> anyhow::Result<Vec<Table>> {
    // Fetch basic table list
    let table_rows = sqlx::query(
        r#"
        SELECT
            t.table_name,
            pgd.description AS table_comment
        FROM information_schema.tables t
        JOIN pg_class c
            ON c.relname = t.table_name
            AND c.relkind = 'r'
        JOIN pg_namespace n
            ON n.oid = c.relnamespace
            AND n.nspname = t.table_schema
        LEFT JOIN pg_description pgd
            ON pgd.objoid = c.oid
            AND pgd.objsubid = 0
        WHERE t.table_schema = 'public'
        AND t.table_type = 'BASE TABLE'
        ORDER BY t.table_name;
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut tables = Vec::new();
    for row in table_rows {
        let table_name: String = row.get("table_name");
        let table_comment: Option<String> = row.get("table_comment");
        
        // columns
        let cols = sqlx::query(
            r#"
            SELECT column_name, udt_name, is_nullable, column_default, character_maximum_length
            FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = $1
            ORDER BY ordinal_position
            "#,
        )
        .bind(&table_name)
        .fetch_all(pool)
        .await?;

        let mut columns = Vec::new();
        for c in cols {
            let mut metadata = HashMap::new();
            let column_default: Option<String> = c.get("column_default");

            if let Some(default) = &column_default {
                // Enum default looks like: `'pending'::order_status`
                if default.contains("::") {
                    // pull name before :: e.g. 'pending'::order_status -> pending
                    if let Some(val) = default.split("::").next() {
                        let v = val.trim_matches('\'');
                        metadata.insert("default-value".to_string(), v.to_string());
                    }
                }

                // Auto Increment: Detect typical Postgres auto-increment sequence default
                if default.starts_with("nextval(") {
                    metadata.insert("default-value".to_string(), "$nextval".to_string());
                }
                // Auto UUID: pgcrypto
                else if default.contains("gen_random_uuid()") {
                    metadata.insert("default-value".to_string(), "$uuid".to_string());
                }
                // Auto UUID: uuid-ossp
                else if default.contains("uuid_generate_v4()") {
                    metadata.insert("default-value".to_string(), "$uuid".to_string());
                }
                // Auto timestamp (CURRENT_TIMESTAMP / now())
                else if default.contains("now()") || default.contains("CURRENT_TIMESTAMP") {
                    metadata.insert("default-value".to_string(), "$now".to_string());
                }
            }

            columns.push(Column {
                name: c.get("column_name"),
                r#type: c.get("udt_name"),
                is_nullable: c.get::<String, _>("is_nullable") == "YES",
                column_default,
                metadata,
            });
        }

        // primary key columns
        let pk_rows = sqlx::query(
            r#"
            SELECT kcu.column_name
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
                ON tc.constraint_name = kcu.constraint_name
                AND tc.table_schema = kcu.table_schema
            WHERE tc.constraint_type = 'PRIMARY KEY'
                AND tc.table_schema = 'public'
                AND tc.table_name = $1
            ORDER BY kcu.ordinal_position
            "#,
        )
        .bind(&table_name)
        .fetch_all(pool)
        .await?;

        let primary_key = pk_rows
            .into_iter()
            .map(|r| r.get("column_name"))
            .collect::<Vec<_>>();

        // unique constraints (each constraint -> list of columns)
        let uc_rows = sqlx::query(
            r#"
            SELECT tc.constraint_name, kcu.column_name
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
                ON tc.constraint_name = kcu.constraint_name
                AND tc.table_schema = kcu.table_schema
            WHERE tc.constraint_type = 'UNIQUE'
                AND tc.table_schema = 'public'
                AND tc.table_name = $1
            ORDER BY tc.constraint_name, kcu.ordinal_position
            "#,
        )
        .bind(&table_name)
        .fetch_all(pool)
        .await?;

        let mut ucs_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in uc_rows {
            ucs_map
                .entry(r.get("constraint_name"))
                .or_default()
                .push(r.get("column_name"));
        }
        let unique_constraints = ucs_map.into_values().collect::<Vec<_>>();

        tables.push(Table {
            name: table_name,
            columns,
            primary_key,
            metadata: if let Some(comment) = table_comment {
                parse_metadata(comment)
            } else {
                HashMap::new()
            },
            unique_constraints,
        });
    }

    Ok(tables)
}

async fn fetch_foreign_keys(pool: &PgPool) -> anyhow::Result<Vec<FKRow>> {
    // Query that returns one row per fk column (composite fks produce multiple rows with same constraint_name)
    let rows = sqlx::query(
        r#"
        SELECT
            tc.constraint_name,
            tc.table_schema,
            tc.table_name,
            kcu.column_name,
            ccu.table_schema AS foreign_table_schema,
            ccu.table_name AS foreign_table_name,
            ccu.column_name AS foreign_column_name,
            kcu.ordinal_position
        FROM information_schema.table_constraints AS tc
        JOIN information_schema.key_column_usage AS kcu
            ON tc.constraint_name = kcu.constraint_name
            AND tc.table_schema = kcu.table_schema
        JOIN information_schema.constraint_column_usage AS ccu
            ON ccu.constraint_name = tc.constraint_name
        WHERE tc.constraint_type = 'FOREIGN KEY'
            AND tc.table_schema='public'
        ORDER BY tc.table_name, tc.constraint_name, kcu.ordinal_position
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut fks = Vec::new();
    for r in rows {
        fks.push(FKRow {
            constraint_name: r.get("constraint_name"),
            table_schema: r.get("table_schema"),
            table_name: r.get("table_name"),
            column_name: r.get("column_name"),
            foreign_table_schema: r.get("foreign_table_schema"),
            foreign_table_name: r.get("foreign_table_name"),
            foreign_column_name: r.get("foreign_column_name"),
            ordinal_position: r.get::<Option<i32>, _>("ordinal_position").unwrap_or(0),
        });
    }
    Ok(fks)
}

// ---------------------------------------------------------------
// COMMENTS → METADATA
// ---------------------------------------------------------------

async fn load_table_comments(pool: &PgPool) -> sqlx::Result<HashMap<String, String>> {
    let rows = sqlx::query(
        r#"
        SELECT c.relname AS table_name, pgd.description AS comment
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        LEFT JOIN pg_description pgd ON pgd.objoid = c.oid AND pgd.objsubid = 0
        WHERE n.nspname='public' AND c.relkind='r'
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let table_name: String = r.get("table_name");
            let comment: Option<String> = r.get("comment");
            comment.map(|c| (table_name, c))
        })
        .collect())
}

async fn load_column_comments(pool: &PgPool) -> sqlx::Result<HashMap<(String, String), String>> {
    let rows = sqlx::query(
        r#"
        SELECT c.relname AS table_name,
               a.attname AS column_name,
               pgd.description AS comment
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_attribute a ON a.attrelid = c.oid
        LEFT JOIN pg_description pgd ON pgd.objoid = a.attrelid AND pgd.objsubid = a.attnum
        WHERE n.nspname='public'
          AND c.relkind='r'
          AND a.attnum > 0
          AND NOT a.attisdropped
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let table_name: String = r.get("table_name");
            let column_name: String = r.get("column_name");
            let comment: Option<String> = r.get("comment");
            comment.map(|c| ((table_name, column_name), c))
        })
        .collect())
}

fn parse_metadata(comment: String) -> HashMap<String, String> {
    comment
        .split(';')
        .filter_map(|part| {
            let mut it = part.splitn(2, '=');
            let key = it.next()?.trim();
            let value = it.next()?.trim();
            if key.is_empty() {
                None
            } else {
                Some((key.to_string(), value.to_string()))
            }
        })
        .collect()
}

fn generate_plantuml(schema: &SchemaDump) -> String {
    // Produce a PlantUML class diagram with simple association arrows and fields
    let mut out = String::new();
    out.push_str("@startuml\n' generated by pg_schema_infer\nhide circle\nskinparam classAttributeIconSize 0\n\n");
    // define classes
    for t in &schema.tables {
        out.push_str(&format!("class {} {{\n", sanitize_identifier(&t.name)));
        for c in &t.columns {
            let nullable_marker = if c.is_nullable { "?" } else { "" };
            out.push_str(&format!(
                "  {} : {}{}\n",
                sanitize_identifier(&c.name),
                c.r#type,
                nullable_marker
            ));
        }
        out.push_str("}\n\n");
    }

    // relationships
    out.push_str("' relationships\n");
    for rel in &schema.relationships {
        match rel.kind.as_str() {
            "one-to-many" => {
                // from_table (child) -> to_table (parent): many -> one
                let child = sanitize_identifier(&rel.from_table);
                let parent = sanitize_identifier(&rel.to_table);
                out.push_str(&format!(
                    "{} \"*\" -- \"1\" {} : \"{} -> {}\"\n",
                    child,
                    parent,
                    rel.from_columns.join(","),
                    rel.to_columns.join(",")
                ));
            }
            "one-to-one" => {
                let a = sanitize_identifier(&rel.from_table);
                let b = sanitize_identifier(&rel.to_table);
                out.push_str(&format!(
                    "{} \"1\" -- \"1\" {} : \"{} -> {}\"\n",
                    a,
                    b,
                    rel.from_columns.join(","),
                    rel.to_columns.join(",")
                ));
            }
            "many-to-many" => {
                // annotate via_table
                let a = sanitize_identifier(&rel.from_table);
                let b = sanitize_identifier(&rel.to_table);
                if let Some(via) = &rel.via_table {
                    let via_s = sanitize_identifier(via);
                    out.push_str(&format!("{} \"*\" -- \"*\" {} : \"via {}\"\n", a, b, via_s));
                } else {
                    out.push_str(&format!(
                        "{} \"*\" -- \"*\" {} : \"{} <-> {}\"\n",
                        a,
                        b,
                        rel.from_columns.join(","),
                        rel.to_columns.join(",")
                    ));
                }
            }
            _ => {}
        }
    }

    out.push_str("\n@enduml\n");
    out
}

fn sanitize_identifier(s: &str) -> String {
    // PlantUML class names: if contains non-word chars, wrap with quotes
    if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        s.to_string()
    } else {
        format!("\"{}\"", s)
    }
}
